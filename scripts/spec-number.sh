#!/bin/sh
# Specification and ADR numbers (ADR-0035 decision 6). A number is reserved before it is used: one
# row in docs/specifications/RESERVED.md on main (number, topic, branch, date), merged through a pull
# request that adds only that row, or added by the coordinator who assigns the number.
#
# Usage:
#   scripts/spec-number.sh next [SPEC|ADR]  prints the next free number (default SPEC): one above
#                                           the highest used by a file or reserved, on the base and
#                                           in the checked-out tree
#   scripts/spec-number.sh check            fails when the branch adds a SPEC-NNNN or ADR-NNNN file
#                                           whose number another file on the base already uses, or
#                                           that RESERVED.md on the base reserves for another branch
#                                           (scripts/check-pr.sh runs it)
#   --base REF                              the base to compare with (default origin/main)
#   scripts/spec-number.sh selftest         runs the script's own tests in a temporary repository
set -eu
cd "$(dirname "$0")/.."
exec python3 -I - "$@" <<'PY'
import pathlib, re, shutil, subprocess, sys, tempfile

RESERVED = 'docs/specifications/RESERVED.md'
PATTERNS = {
    'SPEC': re.compile(r'^docs/specifications/SPEC-(\d{4})-[^/]+\.md$'),
    'ADR': re.compile(r'^docs/adr/(\d{4})-[^/]+\.md$'),
}
ROW = re.compile(r'^\| *(SPEC|ADR)-(\d{4}) *\|([^|]*)\|([^|]*)\|([^|]*)\|')


def git(root, *args, ok=False):
    r = subprocess.run(['git', '-C', str(root), *args], capture_output=True, text=True)
    if r.returncode != 0 and not ok:
        raise SystemExit(f'error: git {" ".join(args)}: {r.stderr.strip()}')
    return r.stdout if r.returncode == 0 else None


def numbered(paths):
    """{(kind, number): {path}} of the specification and ADR files among the paths."""
    out = {}
    for p in paths:
        for kind, pattern in PATTERNS.items():
            m = pattern.match(p)
            if m:
                out.setdefault((kind, int(m.group(1))), set()).add(p)
    return out


def files_at(root, ref):
    text = git(root, 'ls-tree', '-r', '--name-only', ref, '--', 'docs/specifications', 'docs/adr')
    return [l for l in text.splitlines() if l]


def files_now(root):
    tracked = git(root, 'ls-files', '--', 'docs/specifications', 'docs/adr').splitlines()
    others = git(root, 'ls-files', '--others', '--exclude-standard', '--', 'docs/specifications', 'docs/adr').splitlines()
    return [p for p in tracked + others if (root / p).exists()]


def reservations(text):
    """{(kind, number): (topic, branch, date)} of RESERVED.md's table."""
    out = {}
    for line in (text or '').splitlines():
        m = ROW.match(line)
        if m:
            out[(m.group(1), int(m.group(2)))] = tuple(c.strip().strip('`') for c in m.group(3, 4, 5))
    return out


def next_free(root, base, kind):
    used = set(numbered(files_at(root, base))) | set(numbered(files_now(root)))
    used |= set(reservations(git(root, 'show', f'{base}:{RESERVED}', ok=True)))
    path = root / RESERVED
    if path.exists():
        used |= set(reservations(path.read_text(encoding='utf-8')))
    top = max([n for k, n in used if k == kind] or [0])
    return f'{kind}-{top + 1:04d}' if kind == 'SPEC' else f'ADR-{top + 1:04d}'


def check(root, base):
    merge_base = git(root, 'merge-base', base, 'HEAD').strip()
    branch = git(root, 'rev-parse', '--abbrev-ref', 'HEAD').strip()
    on_base = numbered(files_at(root, base))
    at_fork = numbered(files_at(root, merge_base))
    now = numbered(files_now(root))
    reserved = reservations(git(root, 'show', f'{base}:{RESERVED}', ok=True))
    problems, notes = [], []
    # Files this branch renamed or removed: their numbers move with them (a rename is no new number).
    gone = {p for ps in at_fork.values() for p in ps} - {p for ps in now.values() for p in ps}
    for (kind, n), paths in sorted(now.items()):
        new = paths - at_fork.get((kind, n), set())
        if not new:
            continue
        name = f'{kind}-{n:04d}'
        if len(paths) > 1:
            problems.append(f'{name}: used by {len(paths)} files in this tree: {", ".join(sorted(paths))}')
        others = on_base.get((kind, n), set()) - paths - gone
        if others:
            problems.append(f'{name}: {", ".join(sorted(new))} takes a number {base} already uses for '
                            f'{", ".join(sorted(others))}; take a free one (scripts/spec-number.sh next {kind})')
        r = reserved.get((kind, n))
        allowed = [b.strip().strip('`') for b in r[1].split(',')] if r else []
        if r and r[1] and branch not in allowed and branch != 'HEAD':
            problems.append(f'{name}: reserved in {RESERVED} for branch {r[1]} ({r[0]}, {r[2]}), not for {branch}')
        if not r and not others and not at_fork.get((kind, n), set()) & gone:
            notes.append(f'{name}: not reserved in {RESERVED} on {base}; reserve it before others take it')
    return problems, notes


def selftest():
    tmp = pathlib.Path(tempfile.mkdtemp())
    try:
        def run(*args):
            subprocess.run(['git', '-C', str(tmp), *args], check=True, capture_output=True)
        def write(path, text='x\n'):
            p = tmp / path
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text)
        run('init', '-q', '-b', 'main')
        run('config', 'user.email', 'test@example.invalid')
        run('config', 'user.name', 'test')
        write('docs/specifications/SPEC-0001-a.md')
        write('docs/adr/0001-a.md')
        write(RESERVED, '| Number | Topic | Branch | Date |\n|---|---|---|---|\n'
                        '| SPEC-0003 | c | other | 2026-11-18 |\n| SPEC-0004 | d | base, mine | 2026-11-18 |\n')
        run('add', '-A'); run('commit', '-qm', 'base')
        run('checkout', '-qb', 'mine')
        # The main branch moves on: SPEC-0002 lands there.
        run('checkout', '-q', 'main')
        write('docs/specifications/SPEC-0002-main.md'); run('add', '-A'); run('commit', '-qm', 'main')
        run('checkout', '-q', 'mine')
        assert next_free(tmp, 'main', 'SPEC') == 'SPEC-0005', next_free(tmp, 'main', 'SPEC')
        assert next_free(tmp, 'main', 'ADR') == 'ADR-0002'
        assert check(tmp, 'main') == ([], [])
        write('docs/specifications/SPEC-0002-mine.md')   # used on main by another file
        write('docs/specifications/SPEC-0003-mine.md')   # reserved for another branch
        write('docs/specifications/SPEC-0004-mine.md')   # reserved for this branch
        write('docs/adr/0002-mine.md')                   # free, not reserved
        problems, notes = check(tmp, 'main')
        assert len(problems) == 2, problems
        assert any('SPEC-0002' in p and 'SPEC-0002-main.md' in p for p in problems), problems
        assert any('SPEC-0003' in p and 'branch other' in p for p in problems), problems
        assert notes == [f'ADR-0002: not reserved in {RESERVED} on main; reserve it before others take it'], notes
        # An existing spec edited on the branch is not new.
        write('docs/specifications/SPEC-0001-a.md', 'changed\n')
        assert len(check(tmp, 'main')[0]) == 2
        print('selftest: ok')
    finally:
        shutil.rmtree(tmp)


args = sys.argv[1:]
base = 'origin/main'
if '--base' in args:
    i = args.index('--base')
    if i + 1 >= len(args):
        raise SystemExit('error: --base needs a ref')
    base = args[i + 1]
    del args[i:i + 2]
root = pathlib.Path('.').resolve()
cmd = args[0] if args else ''
if cmd == 'next':
    kind = (args[1] if len(args) > 1 else 'SPEC').upper()
    if kind not in PATTERNS:
        raise SystemExit('usage: scripts/spec-number.sh next [SPEC|ADR]')
    print(next_free(root, base, kind))
elif cmd == 'check':
    problems, notes = check(root, base)
    for n in notes:
        print(f'note: {n}')
    for p in problems:
        print(f'error: {p}', file=sys.stderr)
    if not problems:
        print('specification and ADR numbers: ok')
    sys.exit(1 if problems else 0)
elif cmd == 'selftest':
    selftest()
else:
    print('usage: scripts/spec-number.sh [--base REF] next [SPEC|ADR] | check | selftest', file=sys.stderr)
    sys.exit(2)
PY
