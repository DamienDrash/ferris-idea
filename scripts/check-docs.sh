#!/bin/sh
# Checks public documentation: relative links resolve, banned claims are absent,
# the README keeps its independence notice. Usage: scripts/check-docs.sh
set -eu
cd "$(dirname "$0")/.."
python3 -I - <<'PY'
import pathlib, re, sys
root = pathlib.Path('.')
files = [p for p in root.rglob('*.md') if not any(part in ('target', '.git', 'node_modules') for part in p.parts)
         and p.relative_to(root).parts[:1] != ('vendor',)]  # third-party texts, kept as published
errors = []
link = re.compile(r'\]\(([^)\s]+)\)|(?:src|href)="([^"]+)"')
for f in files:
    # A public replacement docs/public/overrides/<path> is read from the place it takes in the
    # public tree, so its links are written for that place.
    rel = f.relative_to(root).parts
    here = root.joinpath(*rel[3:]).parent if rel[:3] == ('docs', 'public', 'overrides') else f.parent
    text = f.read_text(encoding='utf-8')
    # Links quoted as code (fenced blocks and inline `…` spans) are examples, not links.
    text = re.sub(r'```.*?```', '', text, flags=re.S)
    text = re.sub(r'`[^`\n]*`', '', text)
    for m in link.finditer(text):
        target = m.group(1) or m.group(2)
        if re.match(r'^(https?:|mailto:|#)', target):
            continue
        path = target.split('#')[0]
        if not path:
            continue
        if not (here / path).exists():
            errors.append(f'{f}: broken link {target}')
banned = ['world\'s fastest', 'blazing', 'revolutionary', 'perfect replacement', '100% compatible', 'production-ready', 'production ready']
public = [root / 'README.md', root / 'CHANGELOG.md', root / 'CONTRIBUTING.md'] + list((root / 'docs' / 'design').glob('*.md'))
for f in public:
    text = f.read_text(encoding='utf-8').lower()
    for word in banned:
        for m in re.finditer(re.escape(word), text):
            line = text[:m.start()].count('\n') + 1
            context = text.splitlines()[line - 1]
            if 'banned' in context or 'avoid' in context or 'not used' in context or '"' in context or 'bad:' in context:
                continue
            errors.append(f'{f}:{line}: banned claim "{word}"')
readme = (root / 'README.md').read_text(encoding='utf-8')
if 'not affiliated with, endorsed by, or' not in readme:
    errors.append('README.md: independence notice missing')
# The changelog keeps `## Unreleased` above the versions, and its newest version is the one in
# Cargo.toml (CONTENT_STYLE_GUIDE.md §7).
changelog = (root / 'CHANGELOG.md').read_text(encoding='utf-8')
cargo = re.search(r'\[workspace\.package\]\s*\nversion = "([^"]+)"', (root / 'Cargo.toml').read_text(encoding='utf-8'))
releases = re.findall(r'^## IDECraft v(\S+) — \d{4}-\d{2}-\d{2}$', changelog, flags=re.M)
if '\n## Unreleased\n' not in changelog:
    errors.append('CHANGELOG.md: "## Unreleased" section missing')
elif releases and changelog.index('\n## Unreleased\n') > changelog.index(f'## IDECraft v{releases[0]} '):
    errors.append('CHANGELOG.md: "## Unreleased" must come before the versions')
if not cargo or not releases or releases[0] != cargo.group(1):
    errors.append(f'CHANGELOG.md: newest version {releases[0] if releases else None} differs from Cargo.toml {cargo.group(1) if cargo else None}')
for e in errors:
    print(e)
print(f'checked {len(files)} files, {len(errors)} problem(s)')
sys.exit(1 if errors else 0)
PY
# Changelog fragments keep the format of CONTENT_STYLE_GUIDE.md §7, and the script that merges
# them into CHANGELOG.md passes its own tests.
scripts/changelog.sh check
scripts/changelog.sh selftest >/dev/null || { scripts/changelog.sh selftest; exit 1; }
# The crate diagram in ARCHITECTURE.md matches the workspace.
scripts/crate-graph.sh --check >/dev/null
# Every feature-matrix row must be rated in docs/research/PARITY.md.
scripts/parity.sh >/dev/null
# No tracked file keeps the markers of an unfinished merge (2026-11-16: a spec reached main with them).
if git grep -n -I -E '^(<<<<<<< |>>>>>>> )' -- . ':!scripts/check-docs.sh' >/dev/null 2>&1; then
  echo "error: conflict markers left in tracked files:" >&2
  git grep -n -I -E '^(<<<<<<< |>>>>>>> )' -- . ':!scripts/check-docs.sh' >&2
  exit 1
fi
