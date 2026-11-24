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
# A fragment may be named after its branch (`changelog.d/<branch>.md`) and write `(#PR)`; it never
# needs renaming (decision of 2026-11-23): `assemble` finds the number in the subject of the squash
# commit on main that added the file ("… (#123)"). A second push only to write the number started
# the server CI's check of the pull request again (268 pull requests up to 2026-11-23).
# A numbered fragment may still write `(#PR)`: it stands for the number of its file, and
# `assemble` writes that number. So a plain rename (`git mv`) is enough, and a numbering commit
# that kept only the rename but lost the text change (#975 and #980 on 2026-11-17, whose server
# CI failed with "the line names no #N") can no longer break the check.
#
# Usage:
#   scripts/changelog.sh check            checks every fragment (scripts/check-docs.sh runs it)
#   scripts/changelog.sh assemble         moves every fragment under `## Unreleased` in
#                                         CHANGELOG.md, newest pull request first in each group,
#                                         and deletes it; run first at the daily version cut (a
#                                         branch fragment takes the number of the squash commit
#                                         that added it to main)
#   scripts/changelog.sh number <name> <N>
#                                         renames changelog.d/<name>.md to <N>.md and writes N
#                                         for `#PR` (not needed any more, see above)
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
        elif numbered and '#PR' in refs.group(1) and f'#{stem}' in re.findall(r'#[0-9]+', refs.group(1)):
            problems.append(f'{where}: the line names #{stem} and "#PR", which stands for #{stem}')
        elif numbered and '#PR' not in refs.group(1) and f'#{stem}' not in re.findall(r'#[0-9]+', refs.group(1)):
            problems.append(f'{where}: the line names no #{stem} (or "#PR"), the number of its file')
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


SQUASH = re.compile(r'\(#([1-9][0-9]*)\)\s*$')


def merged_number(root, path):
    """The pull request number of a branch fragment: from the subject of the newest commit of HEAD
    that added the file, a squash merge whose subject ends with "(#N)". None when there is none."""
    rel = path.relative_to(root).as_posix() if path.is_absolute() else path.as_posix()
    try:
        out = subprocess.run(['git', 'log', '-1', '--diff-filter=A', '--format=%s', 'HEAD', '--', rel],
                             cwd=root, capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None
    m = SQUASH.search(out)
    return m.group(1) if m else None


def assemble(root):
    paths = fragments(root)
    numbers, missing = {}, []
    for p in paths:
        if p.stem.isdigit():
            numbers[p] = p.stem
        else:
            n = merged_number(root, p)
            if n is None:
                missing.append(p)
            else:
                numbers[p] = n
    if missing:
        for p in missing:
            print(f'{p}: no pull request number: no squash commit "… (#N)" on this branch added it '
                  f'(scripts/changelog.sh number {p.stem} <N>)')
        return 1
    parsed = []
    for p in paths:
        groups, problems = parse(p)
        if problems:
            for q in problems:
                print(q)
            return 1
        n = numbers[p]
        if not p.stem.isdigit() and any(f'#{n}' in l for ls in groups.values() for l in ls):
            print(f'{p}: names #{n} and "#PR", which stands for #{n}')
            return 1
        # `#PR` stands for the number of the fragment's pull request.
        parsed.append((int(n), {g: [l.replace('#PR', f'#{n}') for l in ls] for g, ls in groups.items()}))
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
                       '## Ferris IDEA v0.1.0 — 2026-11-01\n\nStage: Alpha\n\n### Added\n- First (#1)\n', encoding='utf-8')
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
                '## Ferris IDEA v0.1.0 — 2026-11-01\n\nStage: Alpha\n\n### Added\n- First (#1)\n')
        expect(f'assembled changelog:\n{text}', text == want)
        expect('fragments deleted, README kept', [p.name for p in fragments(root)] == [] and (frag / 'README.md').exists())
        expect('assemble without fragments', assemble(root) == 0 and log.read_text(encoding='utf-8') == want)
        bad = {
            '15.md': '### Added\n- No number\n',
            '16.md': '### Added\n- Wrong number (#99)\n',
            '17.md': '### Improved\n- Unknown group (#17)\n',
            '18.md': '- Before a group (#18)\n',
            '19.md': '### Added\n- Placeholder and own number (#19, #PR)\n',
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
        log.write_text('# Changelog\n\n## Unreleased\n\n## Ferris IDEA v0.1.0 — 2026-11-01\n\nStage: Alpha\n', encoding='utf-8')
        expect('assemble into an empty Unreleased', assemble(root) == 0)
        expect('empty Unreleased filled', log.read_text(encoding='utf-8') ==
               '# Changelog\n\n## Unreleased\n\n### Added\n- Thirty (#30)\n\n## Ferris IDEA v0.1.0 — 2026-11-01\n\nStage: Alpha\n')
    merge_selftest(expect)
    for f in failures:
        print(f'FAILED: {f}')
    print(f'changelog.sh selftest: {"ok" if not failures else f"{len(failures)} failure(s)"}')
    return 1 if failures else 0


def merge_selftest(expect):
    """2026-11-17: the numbering commits of #975 and #980 were plain renames that kept "(#PR)";
    the server CI, merging such a branch into main's tip, refused "the line names no #N". Played
    here in a tiny repository: a branch fragment, a plain `git mv` to its number, main assembling
    and deleting other fragments meanwhile, then the merge as the server CI does it."""
    import os
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')

        def git(*a):
            subprocess.run(['git', '-c', 'user.name=selftest', '-c', 'user.email=selftest@example.invalid',
                            '-c', 'core.hooksPath=' + os.devnull, '-c', 'commit.gpgsign=false', *a],
                           cwd=root, env=env, check=True, capture_output=True)

        frag = root / 'changelog.d'
        frag.mkdir()
        (root / 'CHANGELOG.md').write_text('# Changelog\n\n## Unreleased\n\n## Ferris IDEA v0.1.0 — 2026-11-01\n',
                                           encoding='utf-8')
        (frag / '40.md').write_text('### Added\n- Forty, a long line like the others (#40)\n', encoding='utf-8')
        (frag / '41.md').write_text('### Added\n- Forty-one, a long line like the others (#41)\n', encoding='utf-8')
        git('init', '-q', '-b', 'main')
        git('add', '-A')
        git('commit', '-q', '-m', 'base')
        git('checkout', '-q', '-b', 'topic')
        (frag / 'topic.md').write_text('### Added\n- Topic, a long line like the others (#PR)\n', encoding='utf-8')
        git('add', '-A')
        git('commit', '-q', '-m', 'fragment')
        git('mv', 'changelog.d/topic.md', 'changelog.d/42.md')
        git('commit', '-q', '-m', 'only the rename')
        git('checkout', '-q', 'main')
        expect('main assembles', assemble(root) == 0)
        git('add', '-A')
        git('commit', '-q', '-m', 'version cut')
        git('merge', '-q', '--no-edit', 'topic')
        expect('a plainly renamed fragment passes the check after the merge', check(root) == 0)
        expect('assemble after the merge', assemble(root) == 0)
        text = (root / 'CHANGELOG.md').read_text(encoding='utf-8')
        expect(f'"#PR" of 42.md assembled as #42:\n{text}', '- Topic, a long line like the others (#42)\n' in text
               and '#PR' not in text)

    # 2026-11-23: a branch fragment is never renamed; the squash commit that adds it to main names
    # the pull request, and `assemble` takes the number from there.
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1')

        def git(*a):
            subprocess.run(['git', '-c', 'user.name=selftest', '-c', 'user.email=selftest@example.invalid',
                            '-c', 'core.hooksPath=' + os.devnull, '-c', 'commit.gpgsign=false', *a],
                           cwd=root, env=env, check=True, capture_output=True)

        frag = root / 'changelog.d'
        frag.mkdir()
        (root / 'CHANGELOG.md').write_text('# Changelog\n\n## Unreleased\n\n## Ferris IDEA v0.1.0 — 2026-11-01\n',
                                           encoding='utf-8')
        (frag / '50.md').write_text('### Fixed\n- Fifty (#50)\n', encoding='utf-8')
        git('init', '-q', '-b', 'main')
        git('add', '-A')
        git('commit', '-q', '-m', 'base')
        (frag / 'my-topic.md').write_text('### Added\n- Topic without a number (#PR)\n', encoding='utf-8')
        git('add', '-A')
        git('commit', '-q', '-m', 'Topic lands (#51)')
        (frag / 'not-merged.md').write_text('### Added\n- Not on main yet (#PR)\n', encoding='utf-8')
        expect('a branch fragment not added by a squash commit is refused', assemble(root) == 1)
        (frag / 'not-merged.md').unlink()
        expect('a branch fragment takes the number of its squash commit', assemble(root) == 0)
        text = (root / 'CHANGELOG.md').read_text(encoding='utf-8')
        expect(f'branch fragment assembled as #51:\n{text}', '- Topic without a number (#51)\n' in text
               and '- Fifty (#50)\n' in text and '#PR' not in text)
        expect('both fragments deleted', fragments(root) == [])


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
