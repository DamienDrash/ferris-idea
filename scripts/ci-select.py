#!/usr/bin/env python3
"""Which crates a change has to test (ADR-0030): shared by scripts/check-pr.sh and the build server's CI.

    ci-select.py METADATA FILES PLAN [--full] [--changed-only] [--base REF --merge-base SHA] [--quiet]

METADATA is the output of `cargo metadata --format-version 1 --no-deps` of the tree under test,
FILES the changed paths (one per line), PLAN the file this writes:

    mode=full|crates|lint|docs
    crates=<affected crates, space-separated>
    doc_crates=<the affected crates that have a library, for the doctests>
    total=<number of workspace crates>
    why=<why the whole workspace is tested, or nothing>

A file belongs to the crate whose folder is its longest prefix; every workspace crate that depends
on a changed crate, directly or through others, is affected too (normal and build dependencies are
followed all the way; a crate that only has a dev-dependency on an affected crate is added itself).
The whole workspace is tested when the change touches the root Cargo.toml or Cargo.lock, the
toolchain, .cargo/, .config/ (the test runner's settings), vendor/, a build script, the shared
tests/ folder, or a file this script cannot place. Documents only (docs/, changelog.d/, Markdown in
the root): mode docs. Files no crate builds from or reads (scripts/, .github/, …): mode lint (format
and lints, no tests).
"""
import json
import os
import sys

args = sys.argv[1:]
if len(args) < 3:
    sys.exit(__doc__)
meta_path, files_path, plan_path = args[:3]
opts = args[3:]
forced = '--full' in opts
changed_only = '--changed-only' in opts
quiet = '--quiet' in opts
base = opts[opts.index('--base') + 1] if '--base' in opts else 'origin/main'
merge_base = opts[opts.index('--merge-base') + 1] if '--merge-base' in opts else ''

meta = json.load(open(meta_path, encoding='utf-8'))
files = [line.rstrip('\n') for line in open(files_path, encoding='utf-8') if line.strip()]
root = os.path.realpath(meta['workspace_root'])

# Workspace crates: name -> folder relative to the root; folder -> name.
folder = {p['name']: os.path.relpath(os.path.dirname(os.path.realpath(p['manifest_path'])), root)
          for p in meta['packages']}
by_folder = {d: n for n, d in folder.items()}
has_lib = {p['name'] for p in meta['packages']
           if any(k in ('lib', 'rlib', 'proc-macro') for t in p['targets'] for k in t['kind'])}
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


# Files outside the crates that crates read (include_bytes!/include_str! or tests that open them):
# (prefix, crates, dependents too?). Dependents are added when the file is compiled into the crate.
# Not followed: every Markdown file below docs/ is read by one test of ferris-idea-languages (the
# formatter keeps their text); a change of documents alone is checked by the documents check, and
# the full runs of main find a break there.
INPUTS = [
    ('packaging/', ('ferris-idea-ui',), True),                       # icons and desktop entry, built into ui
    ('docs/formats/', ('ferris-idea-core', 'ferris-idea-ui'), False),   # published schemas, compared by tests
    ('scripts/bundle-macos.sh', ('ferris-idea-formats',), False),    # read by a formats test
]
# Root files and folders that force the whole workspace.
WHOLE = ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'rust-toolchain', 'clippy.toml',
         '.clippy.toml', 'rustfmt.toml', '.rustfmt.toml')
WHOLE_DIRS = ('.cargo/', '.config/', 'tests/', 'vendor/')
# Paths no workspace crate builds from or reads (fuzz/ and mobile/ are outside the workspace).
NO_CRATE_DIRS = ('scripts/', '.github/', '.claude/', 'fuzz/', 'mobile/')
NO_CRATE = ('.gitattributes', '.gitignore', 'deny.toml', 'NOTICE')

changed, direct_only, whole, docs, no_crate = set(), set(), [], [], []
for f in files:
    name = os.path.basename(f)
    if f in WHOLE or f.startswith(WHOLE_DIRS):
        whole.append(f)
        continue
    crate = owner(f)
    if crate:
        if name == 'build.rs':
            whole.append(f + ' (build script)')
        else:
            changed.add(crate)
        continue
    if f.startswith('crates/'):
        whole.append(f + ' (in crates/, but in no crate)')
        continue
    for prefix, crates, with_users in INPUTS:
        if f == prefix or (prefix.endswith('/') and f.startswith(prefix)):
            for crate in crates:
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
            affected.add(u)
            todo.append(u)
# A dev-dependency changes the tests of the crate that declares it, not what that crate gives to
# others: such a crate is tested, its own dependents are not added because of it.
for c in list(affected):
    affected |= dev_users[c]
affected |= direct_only

why = ''
if forced:
    mode, why = 'full', '--full'
elif changed_only:
    # The author's quick check: the changed crates alone; what depends on them, and a change this
    # script cannot place, is left to the server's check of the merge result.
    affected = set(changed) | direct_only
    mode = 'crates' if affected else ('lint' if (whole or no_crate) else 'docs')
elif whole:
    mode, why = 'full', ', '.join(whole[:4]) + (f' and {len(whole) - 4} more' if len(whole) > 4 else '')
elif affected:
    mode, why = ('full', 'every crate is affected') if affected == set(folder) else ('crates', '')
elif no_crate:
    mode = 'lint'
else:
    mode = 'docs'
shown = set(affected)
if mode == 'full':
    affected = set(folder)


def names(s):
    return ' '.join(sorted(s)) if s else '(none)'


def some(paths):
    return ', '.join(paths[:4]) + (f' and {len(paths) - 4} more' if len(paths) > 4 else '')


if not quiet:
    print(f'base: {base} (merge base {merge_base[:9]}), {len(files)} changed file(s)')
    print(f'changed crates: {names(changed)}')
    if direct_only - changed:
        print(f'crates that read a changed file outside crates/: {names(direct_only - changed)}')
    print(f'affected crates ({len(shown)} of {len(folder)}): {names(shown)}')
    print(f'whole workspace: yes ({why})' if mode == 'full' else 'whole workspace: no')
    if no_crate:
        print(f'files no crate is tested for: {some(no_crate)}')
    if mode == 'docs':
        if not files:
            print('nothing differs from the merge base: documents check only')
        else:
            print('documents only: documents check only')
    elif mode == 'lint':
        print('no crate is affected: format, lints and documents check only')
with open(plan_path, 'w', encoding='utf-8') as out:
    out.write(f'mode={mode}\n')
    out.write('crates=' + ' '.join(sorted(affected)) + '\n')
    out.write('doc_crates=' + ' '.join(sorted(affected & has_lib)) + '\n')
    out.write(f'total={len(folder)}\n')
    out.write(f'why={why}\n')
