#!/bin/sh
# Writes the crate dependency diagram in ARCHITECTURE.md from `cargo metadata` (workspace crates
# only, normal dependencies, transitive edges left out so the picture stays readable).
# Usage: scripts/crate-graph.sh          rewrite the block between the markers
#        scripts/crate-graph.sh --check  fail when the block differs from the workspace
set -eu
cd "$(dirname "$0")/.."
meta="${TMPDIR:-/tmp}/ferris-idea-metadata.$$.json"
trap 'rm -f "$meta"' EXIT
cargo metadata --format-version 1 --no-deps >"$meta"
python3 -I - "$meta" "${1:-}" <<'PY'
import json, pathlib, sys
meta = json.load(open(sys.argv[1], encoding='utf-8'))
check = sys.argv[2] == '--check'
def short(name):
    return 'app' if name == 'ferris-idea' else name.removeprefix('ferris-idea-')
names = {p['name'] for p in meta['packages']}
deps = {short(p['name']): {short(d['name']) for d in p['dependencies']
                           if d['name'] in names and d['kind'] is None}
        for p in meta['packages']}
def reach(start):
    seen, todo = set(), list(deps[start])
    while todo:
        n = todo.pop()
        if n not in seen:
            seen.add(n)
            todo.extend(deps[n])
    return seen
edges = []
for a in sorted(deps):
    for b in sorted(deps[a]):
        if not any(b in reach(c) for c in deps[a] if c != b):
            edges.append((a, b))
lines = ['```mermaid', 'graph TD']
lines += [f'  {a} --> {b}' for a, b in edges]
lines += [f'  {n}' for n in sorted(deps) if not any(n in e for e in edges)]
lines.append('```')
block = '\n'.join(lines)
path = pathlib.Path('ARCHITECTURE.md')
text = path.read_text(encoding='utf-8')
begin, end = '<!-- crate-graph:begin -->', '<!-- crate-graph:end -->'
head, rest = text.split(begin)
_, tail = rest.split(end)
new = f'{head}{begin}\n{block}\n{end}{tail}'
if check:
    if new != text:
        sys.exit('ARCHITECTURE.md: crate diagram differs from the workspace; run scripts/crate-graph.sh')
else:
    path.write_text(new, encoding='utf-8')
print(f'{len(deps)} crates, {len(edges)} direct edges shown')
PY
