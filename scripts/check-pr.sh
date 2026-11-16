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
# toolchain, .cargo/, a build script, the shared tests/ folder, or a file this script cannot place.
# Documents only (docs/, changelog.d/, Markdown in the root): scripts/check-docs.sh alone.
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

tmp=$(mktemp -d "${TMPDIR:-/tmp}/idecraft-check-pr.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
{
  git -c core.quotepath=off diff --name-only --no-renames "$merge_base"
  git -c core.quotepath=off ls-files --others --exclude-standard
} | sort -u >"$tmp/files"
cargo metadata --format-version 1 --no-deps >"$tmp/metadata.json"

# Writes $tmp/plan (mode=…, crates=…) and prints the report.
python3 -I - "$tmp/metadata.json" "$tmp/files" "$tmp/plan" "$full" "$base" "$merge_base" "$changed_only" <<'PY'
import json, os, sys
meta = json.load(open(sys.argv[1], encoding='utf-8'))
files = [l.rstrip('\n') for l in open(sys.argv[2], encoding='utf-8') if l.strip()]
plan_path, forced, base, merge_base = sys.argv[3], sys.argv[4] == '1', sys.argv[5], sys.argv[6]
changed_only = sys.argv[7] == '1'
root = os.path.realpath(meta['workspace_root'])

# Workspace crates: name -> folder relative to the root; folder -> name.
folder = {p['name']: os.path.relpath(os.path.dirname(os.path.realpath(p['manifest_path'])), root)
          for p in meta['packages']}
by_folder = {d: n for n, d in folder.items()}
# users[x] = workspace crates that build against x (normal or build dependency); dev_users[x] =
# crates whose tests alone use x. A path dependency is matched by its folder (it may be renamed),
# anything else by its name.
users = {n: set() for n in folder}
dev_users = {n: set() for n in folder}
for p in meta['packages']:
    for d in p['dependencies']:
        target = None
        if d.get('path'):
            target = by_folder.get(os.path.relpath(os.path.realpath(d['path']), root))
        elif d['name'] in folder and d.get('source') is None:
            target = d['name']
        if target and target != p['name']:
            (dev_users if d.get('kind') == 'dev' else users)[target].add(p['name'])

def owner(path):
    """The crate whose folder is the longest prefix of the path, or None."""
    best = None
    for d, n in by_folder.items():
        if d != '.' and (path == d or path.startswith(d + '/')):
            if best is None or len(d) > len(folder[best]):
                best = n
    return best

# Files outside the crates that crates read (include_bytes!/include_str! or tests that open them).
# (prefix, crate, dependents too?) Dependents are added when the file is compiled into the crate.
INPUTS = [
    ('packaging/', 'idecraft-ui', True),              # icons and desktop entry, built into ui
    ('docs/formats/', 'idecraft-core', False),        # published schemas, compared by a core test
    ('scripts/bundle-macos.sh', 'idecraft-formats', False),  # read by a formats test
]
# Root files and folders that force the whole workspace.
WHOLE = ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'rust-toolchain', 'clippy.toml',
         '.clippy.toml', 'rustfmt.toml', '.rustfmt.toml')
WHOLE_DIRS = ('.cargo/', 'tests/')
# Paths no workspace crate builds from or reads (fuzz/ and mobile/ are outside the workspace).
NO_CRATE_DIRS = ('scripts/', '.github/', '.claude/', 'fuzz/', 'mobile/')
NO_CRATE = ('.gitattributes', '.gitignore', 'deny.toml', 'NOTICE')

changed, direct_only, whole, docs, no_crate = set(), set(), [], [], []
for f in files:
    name = os.path.basename(f)
    if f in WHOLE or f.startswith(WHOLE_DIRS):
        whole.append(f); continue
    crate = owner(f)
    if crate:
        if name == 'build.rs':
            whole.append(f + ' (build script)')
        else:
            changed.add(crate)
        continue
    if f.startswith('crates/'):
        whole.append(f + ' (in crates/, but in no crate)'); continue
    for prefix, crate, with_users in INPUTS:
        if f == prefix or (prefix.endswith('/') and f.startswith(prefix)):
            if crate not in folder:
                whole.append(f + f' (read by {crate}, which is not in the workspace)')
            elif with_users:
                changed.add(crate)
            else:
                direct_only.add(crate)
            break
    else:
        if f.startswith(('docs/', 'changelog.d/')) or ('/' not in f and f.endswith('.md')):
            docs.append(f)
        elif f.startswith(NO_CRATE_DIRS) or f in NO_CRATE or name.startswith('LICENSE'):
            no_crate.append(f)
        else:
            whole.append(f + ' (not known to this script)')

affected, todo = set(changed), list(changed)
while todo:
    for u in users[todo.pop()]:
        if u not in affected:
            affected.add(u); todo.append(u)
# A dev-dependency changes the tests of the crate that declares it, not what that crate gives to
# others: such a crate is tested, its own dependents are not added because of it.
for c in list(affected):
    affected |= dev_users[c]
affected |= direct_only

if forced:
    mode, why = 'full', '--full'
elif changed_only:
    # The author's quick check: the changed crates alone; what depends on them, and a change this
    # script cannot place, is left to the server's check of the merge result.
    affected = set(changed) | direct_only
    mode, why = ('crates', '') if affected else ('lint' if (whole or no_crate) else 'docs', '')
elif whole:
    mode, why = 'full', ', '.join(whole[:4]) + (f' and {len(whole) - 4} more' if len(whole) > 4 else '')
elif affected:
    mode, why = ('full', 'every crate is affected') if affected == set(folder) else ('crates', '')
else:
    mode, why = 'docs', ''

def names(s):
    return ' '.join(sorted(s)) if s else '(none)'
def some(paths):
    return ', '.join(paths[:4]) + (f' and {len(paths) - 4} more' if len(paths) > 4 else '')
print(f'base: {base} (merge base {merge_base[:9]}), {len(files)} changed file(s)')
print(f'changed crates: {names(changed)}')
if direct_only - changed:
    print(f'crates that read a changed file outside crates/: {names(direct_only - changed)}')
print(f'affected crates ({len(affected)} of {len(folder)}): {names(affected)}')
if mode == 'full':
    print(f'whole workspace: yes ({why})')
else:
    print('whole workspace: no')
if no_crate:
    print(f'files no crate is tested for: {some(no_crate)}')
if mode == 'docs':
    if not files:
        print('nothing differs from the merge base: documents check only')
    elif not no_crate:
        print('documents only: documents check only')
    else:
        print('no crate is affected: documents check only')
with open(plan_path, 'w', encoding='utf-8') as out:
    out.write(f'mode={mode}\n')
    out.write('crates=' + ' '.join(sorted(affected)) + '\n')
PY

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
printf '\n== summary: all steps passed\n%s' "$summary"
