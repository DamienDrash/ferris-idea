#!/bin/sh
# Traceability fragments (ADR-0035 decision 5): a pull request adds its rows of
# docs/specifications/TRACEABILITY.md as docs/specifications/traceability.d/<PR number or branch>.md
# instead of appending them to the end of TRACEABILITY.md, where every other pull request appends
# too. A fragment holds table rows only, in the format of TRACEABILITY.md:
#
#   | SPEC-0338/AK-1 | `crates/core/src/check_gates.rs` (`default_gates`) | `check_gates::tests::default_gates_per_build_system` |
#
# Usage:
#   scripts/traceability.sh check      checks every fragment (scripts/check-docs.sh runs it): rows
#                                      only, a spec that exists, no key given twice in TRACEABILITY.md
#                                      and the fragments together
#   scripts/traceability.sh assemble   appends the rows of every fragment to TRACEABILITY.md (fragments
#                                      in name order, numbers first) and deletes them; run at the
#                                      daily version cut, after scripts/changelog.sh assemble
#   scripts/traceability.sh selftest   runs the script's own tests in a temporary folder
set -eu
cd "$(dirname "$0")/.."
exec python3 -I - "$@" <<'PY'
import pathlib, re, shutil, sys, tempfile

NAME = re.compile(r'^(?:[1-9][0-9]*|[a-z0-9][a-z0-9._-]*)$')
ROW = re.compile(r'^\| *(SPEC-(\d{4})/AK-[^|]*?) *\|')


def key(cell):
    return re.sub(r'  +', ' ', cell.strip())


def fragments(folder):
    files = [p for p in folder.glob('*.md') if p.name != 'README.md']
    return sorted(files, key=lambda p: (not p.stem.isdigit(), int(p.stem) if p.stem.isdigit() else 0, p.stem))


def cells(line):
    """The number of cells of a table row; `\\|` inside a cell does not split it."""
    return len(re.split(r'(?<!\\)\|', line.strip())) - 2


def check(root):
    spec_dir = root / 'docs' / 'specifications'
    folder = spec_dir / 'traceability.d'
    problems, seen = [], {}
    for n, line in enumerate((spec_dir / 'TRACEABILITY.md').read_text(encoding='utf-8').splitlines(), 1):
        m = ROW.match(line)
        if m:
            seen.setdefault(key(m.group(1)), f'TRACEABILITY.md:{n}')
    specs = {p.name[5:9] for p in spec_dir.glob('SPEC-*.md')}
    for path in fragments(folder) if folder.is_dir() else []:
        if not NAME.match(path.stem):
            problems.append(f'{path}: name it <PR number>.md, or <branch>.md before the PR exists')
        for n, line in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
            where = f'{path}:{n}'
            if not line.strip():
                continue
            m = ROW.match(line)
            if not m:
                problems.append(f'{where}: not a row "| SPEC-NNNN/AK-N | implementation | tests |"')
                continue
            if cells(line) != 3:
                problems.append(f'{where}: a row has three cells (spec/AK, implementation, tests)')
            if m.group(2) not in specs:
                problems.append(f'{where}: no docs/specifications/SPEC-{m.group(2)}-*.md')
            k = key(m.group(1))
            if k in seen:
                problems.append(f'{where}: duplicate row {k} (first at {seen[k]})')
            else:
                seen[k] = where
    return problems


def assemble(root):
    spec_dir = root / 'docs' / 'specifications'
    folder = spec_dir / 'traceability.d'
    problems = check(root)
    if problems:
        return problems
    files = fragments(folder) if folder.is_dir() else []
    rows = [l for p in files for l in p.read_text(encoding='utf-8').splitlines() if l.strip()]
    if rows:
        target = spec_dir / 'TRACEABILITY.md'
        text = target.read_text(encoding='utf-8')
        if not text.endswith('\n'):
            text += '\n'
        target.write_text(text + '\n'.join(rows) + '\n', encoding='utf-8')
    for p in files:
        p.unlink()
    print(f'{len(rows)} row(s) from {len(files)} fragment(s) appended to TRACEABILITY.md')
    return []


def selftest():
    tmp = pathlib.Path(tempfile.mkdtemp())
    try:
        spec = tmp / 'docs' / 'specifications'
        (spec / 'traceability.d').mkdir(parents=True)
        (spec / 'SPEC-0001-a.md').write_text('# a\n')
        (spec / 'SPEC-0002-b.md').write_text('# b\n')
        (spec / 'TRACEABILITY.md').write_text('| Spec / AK | Implementation | Test(s) |\n|---|---|---|\n| SPEC-0001/AK-1 | `a` | `t` |\n')
        frag = spec / 'traceability.d'
        (frag / 'README.md').write_text('| SPEC-0009/AK-1 | example | ignored |\n')
        (frag / '12.md').write_text('| SPEC-0002/AK-1 | `b` | `t` |\n')
        (frag / 'some-branch.md').write_text('| SPEC-0002/AK-2 | `b \\| c` | `t` |\n')
        assert check(tmp) == [], check(tmp)
        (frag / 'bad.md').write_text('| SPEC-0001/AK-1 | dup | `t` |\n| SPEC-0003/AK-1 | x | y |\ntext\n| SPEC-0002/AK-3 | two cells |\n')
        p = check(tmp)
        assert any('duplicate row SPEC-0001/AK-1' in x for x in p), p
        assert any('no docs/specifications/SPEC-0003' in x for x in p), p
        assert any('not a row' in x for x in p), p
        assert any('three cells' in x for x in p), p
        (frag / 'bad.md').unlink()
        assert assemble(tmp) == []
        lines = (spec / 'TRACEABILITY.md').read_text().splitlines()
        assert lines[-2:] == ['| SPEC-0002/AK-1 | `b` | `t` |', '| SPEC-0002/AK-2 | `b \\| c` | `t` |'], lines
        assert sorted(p.name for p in frag.iterdir()) == ['README.md']
        print('selftest: ok')
    finally:
        shutil.rmtree(tmp)


cmd = sys.argv[1] if len(sys.argv) > 1 else ''
root = pathlib.Path('.')
if cmd == 'check':
    problems = check(root)
elif cmd == 'assemble':
    problems = assemble(root)
elif cmd == 'selftest':
    selftest()
    problems = []
else:
    print('usage: scripts/traceability.sh check|assemble|selftest', file=sys.stderr)
    sys.exit(2)
for p in problems:
    print(p, file=sys.stderr)
if cmd == 'check' and not problems:
    print('traceability fragments: ok')
sys.exit(1 if problems else 0)
PY
