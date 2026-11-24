#!/bin/sh
# The check before every commit (CLAUDE.md rule 6, ADR-0030): format and lints for the whole
# workspace, tests only for the crates the branch changes and the crates that depend on them. The
# whole workspace is tested by the project's server CI, on the branch merged into
# main's tip, before the branch is merged.
#
#   check-pr.sh                 check the checked-out tree against origin/main
#   check-pr.sh --base REF      compare with the merge base of REF and HEAD instead
#   check-pr.sh --full          test the whole workspace whatever changed
#   check-pr.sh --changed-only  test only the crates the branch changes, not the crates that depend
#                               on them (the author's quick check; the server CI tests the rest on
#                               the merge result before the merge, ADR-0030)
#   check-pr.sh --list          print the changed crates, the affected crates and the commands; run nothing
#
# Changed files are those that differ from the merge base, uncommitted and untracked ones included.
# A file belongs to the crate whose folder is its longest prefix; every workspace crate that depends
# on a changed crate, directly or through others, is tested too (normal and build dependencies are
# followed all the way; a crate that only has a dev-dependency on an affected crate is added itself).
# The whole workspace is tested when the change touches the root Cargo.toml or Cargo.lock, the
# toolchain, .cargo/, .config/, vendor/, a build script, the shared tests/ folder, or a file this
# script cannot place. Documents only (docs/, changelog.d/, Markdown in the root):
# scripts/check-docs.sh alone; files no crate reads (scripts/, .github/, …): format and lints.
# The selection is scripts/ci-select.py, which the server CI uses too. Every run ends with
# scripts/spec-number.sh check: new specifications and ADRs take free numbers.
#
# Steps stop at the first failure; every step ends with one line naming its duration.
set -eu
cd "$(dirname "$0")/.."

base=origin/main
full=0
list=0
changed_only=0
while [ $# -gt 0 ]; do
  case "$1" in
    --base) [ $# -ge 2 ] || { echo "error: --base needs a ref" >&2; exit 2; }; base="$2"; shift ;;
    --full) full=1 ;;
    --list) list=1 ;;
    --changed-only) changed_only=1 ;;
    *) echo "usage: $0 [--base origin/main] [--full] [--changed-only] [--list]" >&2; exit 2 ;;
  esac
  shift
done

merge_base=$(git merge-base "$base" HEAD 2>/dev/null) \
  || { echo "error: no merge base of $base and HEAD (git fetch origin?)" >&2; exit 2; }

tmp=$(mktemp -d "${TMPDIR:-/tmp}/ferris-idea-check-pr.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
{
  git -c core.quotepath=off diff --name-only --no-renames "$merge_base"
  git -c core.quotepath=off ls-files --others --exclude-standard
} | sort -u >"$tmp/files"
cargo metadata --format-version 1 --no-deps >"$tmp/metadata.json"

# Writes $tmp/plan (mode=…, crates=…) and prints the report (the same selection as the server CI).
set --
[ "$full" = 1 ] && set -- "$@" --full
[ "$changed_only" = 1 ] && set -- "$@" --changed-only
python3 -I scripts/ci-select.py "$tmp/metadata.json" "$tmp/files" "$tmp/plan" "$@" \
  --base "$base" --merge-base "$merge_base"

mode=$(sed -n 's/^mode=//p' "$tmp/plan")
crates=$(sed -n 's/^crates=//p' "$tmp/plan")

# The test command as positional parameters.
set -- cargo test
if [ "$mode" = full ]; then
  set -- "$@" --workspace
elif [ "$mode" = lint ]; then
  :
else
  for c in $crates; do set -- "$@" -p "$c"; done
fi
set -- "$@" --no-fail-fast

if [ "$list" = 1 ]; then
  echo "commands:"
  if [ "$mode" != docs ]; then
    echo "  cargo fmt --all --check"
    echo "  cargo clippy --workspace --all-targets -- -D warnings"
    [ "$mode" = lint ] || echo "  $*"
  fi
  echo "  scripts/check-docs.sh"
  echo "  scripts/spec-number.sh --base $base check"
  exit 0
fi

summary=""
# step NAME COMMAND…: runs the command, prints one line with the result and the duration, and ends
# the script with the summary of all steps when the command fails.
step() {
  step_name=$1; shift
  printf '\n== %s\n' "$step_name"
  step_start=$(date +%s)
  step_rc=0
  "$@" || step_rc=$?
  step_secs=$(( $(date +%s) - step_start ))
  step_time="$(( step_secs / 60 ))m$(( step_secs % 60 ))s"
  if [ "$step_rc" -eq 0 ]; then step_line="ok      $step_time  $step_name"
  else step_line="FAILED  $step_time  $step_name (exit $step_rc)"; fi
  echo "-- $step_line"
  summary="$summary$step_line
"
  if [ "$step_rc" -ne 0 ]; then
    printf '\n== summary: failed at %s\n%s' "$step_name" "$summary"
    exit "$step_rc"
  fi
}

echo "commit $(git rev-parse --short HEAD) on $(git rev-parse --abbrev-ref HEAD)"
if [ "$mode" != docs ]; then
  step "cargo fmt --all --check" cargo fmt --all --check
  step "cargo clippy --workspace --all-targets" cargo clippy --workspace --all-targets -- -D warnings
  [ "$mode" = lint ] || step "$*" "$@"
fi
step "scripts/check-docs.sh" scripts/check-docs.sh
# A new specification or ADR takes a number no other file on the base uses and RESERVED.md does not
# reserve for another branch (ADR-0035 decision 6).
step "scripts/spec-number.sh check" scripts/spec-number.sh --base "$base" check
printf '\n== summary: all steps passed\n%s' "$summary"
