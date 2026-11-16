#!/bin/sh
# Changelog fragments (CONTENT_STYLE_GUIDE.md §7, decision of 2026-11-16 in the internal status
# file).
#
# A pull request with a user-visible change adds `changelog.d/<PR number>.md` instead of editing
# CHANGELOG.md, so no two pull requests touch the same file. A fragment holds the lines in the
# format of §7 under their groups, every line ending with its pull request numbers:
#
#   ### Added
#   - Something new you can do (SPEC-0001) (#123)
#   ### Fixed
#   - Something that works again (#123)
#
# A branch pushed before its pull request exists names its fragment after the branch
# (`changelog.d/<branch>.md`) and writes `(#PR)`; `number` gives it the number once it is known.
#
# Usage:
#   scripts/changelog.sh check            checks every fragment (scripts/check-docs.sh runs it)
#   scripts/changelog.sh assemble         moves every numbered fragment under `## Unreleased` in
#                                         CHANGELOG.md, newest pull request first in each group,
#                                         and deletes it; run first at the daily version cut
#   scripts/changelog.sh number <name> <N>
#                                         renames changelog.d/<name>.md to <N>.md and writes N
#                                         for `#PR`
#   scripts/changelog.sh selftest         runs the script's own tests in a temporary folder
set -eu
cd "$(dirname "$0")/.."
exec python3 -I - "$@" <<'PY'
import pathlib, re, shutil, subprocess, sys, tempfile

GROUPS = ['Added', 'Changed', 'Fixed', 'Removed', 'Security']
NAME = re.compile(r'^(?:[1-9][0-9]*|[a-z0-9][a-z0-9._-]*)$')
REFS = re.compile(r'\((#(?:[0-9]+|PR)(?:, #(?:[0-9]+|PR))*)\)\.?$')


def parse(path):
    """The groups of a fragment as {group: [lines]}, or a list of problems."""
    problems, groups, group = [], {}, None
    stem = path.stem
    if path.suffix != '.md' or not NAME.match(stem):
        return None, [f'{path}: name it <PR number>.md, or <branch>.md before the PR exists']
    numbered = stem.isdigit()
    text = path.read_text(encoding='utf-8')
    for i, line in enumerate(text.splitlines(), 1):
        where = f'{path}:{i}'
        if not line.strip():
            continue
        if line.startswith('### '):
            group = line[4:].strip()
            if group not in GROUPS:
                problems.append(f'{where}: unknown group "{group}" (one of {", ".join(GROUPS)})')
                group = None
            elif group in groups:
                problems.append(f'{where}: group "{group}" appears twice')
            else:
                groups[group] = []
            continue
        if not line.startswith('- ') or not line[2:].strip():
            problems.append(f'{where}: a line is "- <change> (#N)" under a "### <group>" heading')
            continue
        if group is None:
            problems.append(f'{where}: the line stands under no known "### <group>" heading')
            continue
        refs = REFS.search(line.rstrip())
        if not refs:
            problems.append(f'{where}: the line must end with its pull request, e.g. "(#123)"')
        elif numbered and f'#{stem}' not in re.findall(r'#[0-9]+', refs.group(1)):
            problems.append(f'{where}: the line names no #{stem}, the number of its file')
        elif numbered and '#PR' in refs.group(1):
            problems.append(f'{where}: "#PR" left in a numbered fragment')
        elif not numbered and '#PR' not in refs.group(1):
            problems.append(f'{where}: a fragment named after its branch writes "(#PR)"')
        groups[group].append(line.rstrip())
    if not problems and not any(groups.values()):
        problems.append(f'{path}: no change in it')
    for g, lines in groups.items():
        if not lines and not problems:
            problems.append(f'{path}: group "{g}" is empty')
    return groups, problems


def fragments(root):
    folder = root / 'changelog.d'
    if not folder.is_dir():
        return []
    return sorted(p for p in folder.iterdir() if p.name != 'README.md')


def check(root):
    problems = []
    for path in fragments(root):
        if path.is_dir():
            problems.append(f'{path}: changelog.d holds files only')
            continue
        problems += parse(path)[1]
    for p in problems:
        print(p)
    print(f'checked {len(fragments(root))} changelog fragment(s), {len(problems)} problem(s)')
    return 1 if problems else 0


def assemble(root):
    paths = fragments(root)
    named = [p for p in paths if not p.stem.isdigit()]
    if named:
        for p in named:
            print(f'{p}: no pull request number yet (scripts/changelog.sh number {p.stem} <N>)')
        return 1
    parsed = []
    for p in paths:
        groups, problems = parse(p)
        if problems:
            for q in problems:
                print(q)
            return 1
        parsed.append((int(p.stem), groups))
    if not parsed:
        print('no changelog fragments')
        return 0
    changelog = root / 'CHANGELOG.md'
    lines = changelog.read_text(encoding='utf-8').split('\n')
    try:
        start = lines.index('## Unreleased')
    except ValueError:
        print('CHANGELOG.md: "## Unreleased" section missing')
        return 1
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith('## ')), len(lines))
    section = lines[start + 1:end]
    # The section as {group: [lines]}, keeping any text before the first group.
    head, have, group = [], {}, None
    for line in section:
        if line.startswith('### ') and line[4:].strip() in GROUPS:
            group = line[4:].strip()
            have.setdefault(group, [])
        elif group is None:
            head.append(line)
        elif line.strip():
            have[group].append(line)
    for g in GROUPS:
        new = [l for _, groups in sorted(parsed, key=lambda x: -x[0]) for l in groups.get(g, [])]
        if new:
            have[g] = new + have.get(g, [])
    body = [l for l in head if l.strip()]
    out = [''] + (body + [''] if body else [])
    for g in GROUPS:
        if have.get(g):
            out += [f'### {g}'] + have[g] + ['']
    lines[start + 1:end] = out
    changelog.write_text('\n'.join(lines), encoding='utf-8')
    for p in paths:
        p.unlink()
    print(f'assembled {len(paths)} fragment(s): ' + ', '.join(f'#{n}' for n, _ in sorted(parsed)))
    return 0


def number(root, name, n):
    if not n.isdigit() or n.startswith('0'):
        print(f'"{n}" is no pull request number')
        return 1
    src, dst = root / 'changelog.d' / f'{name}.md', root / 'changelog.d' / f'{n}.md'
    if not src.is_file():
        print(f'{src}: missing')
        return 1
    if dst.exists():
        print(f'{dst}: exists already')
        return 1
    dst.write_text(src.read_text(encoding='utf-8').replace('#PR', f'#{n}'), encoding='utf-8')
    src.unlink()
    print(f'{src} -> {dst}')
    return check(root)


def selftest():
    failures = []

    def expect(what, cond):
        if not cond:
            failures.append(what)

    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / 'changelog.d').mkdir()
        log = root / 'CHANGELOG.md'
        log.write_text('# Changelog\n\n## Unreleased\n\n### Added\n- Old thing (#10)\n\n### Fixed\n- Old fix (#9)\n\n'
                       '## IDECraft v0.1.0 — 2026-11-01\n\nStage: Alpha\n\n### Added\n- First (#1)\n', encoding='utf-8')
        frag = root / 'changelog.d'
        (frag / '12.md').write_text('### Fixed\n- A fix (SPEC-0001) (#12)\n### Security\n- Safer (#12)\n', encoding='utf-8')
        (frag / '11.md').write_text('### Added\n- New thing (#11, #13)\n### Fixed\n- Fix of 11 (#11)\n', encoding='utf-8')
        (frag / 'README.md').write_text('not a fragment\n', encoding='utf-8')
        expect('check passes good fragments', check(root) == 0)
        (frag / 'my-branch.md').write_text('### Changed\n- Changed thing (#PR)\n', encoding='utf-8')
        expect('a branch fragment with #PR is fine', check(root) == 0)
        expect('assemble refuses an unnumbered fragment', assemble(root) == 1)
        expect('nothing changed by the refusal', '(#12)' not in log.read_text(encoding='utf-8'))
        expect('number renames', number(root, 'my-branch', '14') == 0)
        expect('number wrote the number', (frag / '14.md').read_text(encoding='utf-8') == '### Changed\n- Changed thing (#14)\n')
        expect('assemble works', assemble(root) == 0)
        text = log.read_text(encoding='utf-8')
        want = ('# Changelog\n\n## Unreleased\n\n### Added\n- New thing (#11, #13)\n- Old thing (#10)\n\n'
                '### Changed\n- Changed thing (#14)\n\n'
                '### Fixed\n- A fix (SPEC-0001) (#12)\n- Fix of 11 (#11)\n- Old fix (#9)\n\n'
                '### Security\n- Safer (#12)\n\n'
                '## IDECraft v0.1.0 — 2026-11-01\n\nStage: Alpha\n\n### Added\n- First (#1)\n')
        expect(f'assembled changelog:\n{text}', text == want)
        expect('fragments deleted, README kept', [p.name for p in fragments(root)] == [] and (frag / 'README.md').exists())
        expect('assemble without fragments', assemble(root) == 0 and log.read_text(encoding='utf-8') == want)
        bad = {
            '15.md': '### Added\n- No number\n',
            '16.md': '### Added\n- Wrong number (#99)\n',
            '17.md': '### Improved\n- Unknown group (#17)\n',
            '18.md': '- Before a group (#18)\n',
            '19.md': '### Added\n- Left placeholder (#PR)\n',
            'Branch.md': '### Added\n- Upper-case name (#PR)\n',
            'other.md': '### Added\n- Branch fragment with a number (#20)\n',
            '21.md': '### Added\n',
            '22.txt': '### Added\n- Wrong suffix (#22)\n',
            '23.md': '### Added\n- Twice (#23)\n### Added\n- Again (#23)\n',
            '24.md': '### Added\n  continued line\n',
        }
        for name, body in bad.items():
            for p in fragments(root):
                p.unlink()
            (frag / name).write_text(body, encoding='utf-8')
            expect(f'check refuses {name}', check(root) == 1)
        for p in fragments(root):
            p.unlink()
        (frag / '30.md').write_text('### Added\n- Thirty (#30)\n', encoding='utf-8')
        log.write_text('# Changelog\n\n## Unreleased\n\n## IDECraft v0.1.0 — 2026-11-01\n\nStage: Alpha\n', encoding='utf-8')
        expect('assemble into an empty Unreleased', assemble(root) == 0)
        expect('empty Unreleased filled', log.read_text(encoding='utf-8') ==
               '# Changelog\n\n## Unreleased\n\n### Added\n- Thirty (#30)\n\n## IDECraft v0.1.0 — 2026-11-01\n\nStage: Alpha\n')
    for f in failures:
        print(f'FAILED: {f}')
    print(f'changelog.sh selftest: {"ok" if not failures else f"{len(failures)} failure(s)"}')
    return 1 if failures else 0


args = sys.argv[1:]
root = pathlib.Path('.')
if args == ['check']:
    sys.exit(check(root))
if args == ['assemble']:
    sys.exit(assemble(root))
if len(args) == 3 and args[0] == 'number':
    sys.exit(number(root, args[1], args[2]))
if args == ['selftest']:
    sys.exit(selftest())
print(__doc__ or 'usage: scripts/changelog.sh check | assemble | number <name> <N> | selftest', file=sys.stderr)
sys.exit(2)
PY
