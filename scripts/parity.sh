#!/bin/sh
# Computes feature parity with the reference from docs/research/PARITY.md and checks that every row
# of the feature matrix is rated exactly once. Usage: scripts/parity.sh
set -eu
cd "$(dirname "$0")/.."
python3 -I - <<'PY'
import pathlib, re, sys
share = {0: 0.0, 1: 0.10, 2: 0.30, 3: 0.70, 4: 1.0}
rows = {}
for line in pathlib.Path('docs/research/PARITY.md').read_text(encoding='utf-8').splitlines():
    m = re.match(r'\| (F-\d{3}) \| [^|]+ \| (\d) \| (\d) \|', line)
    if m:
        if m.group(1) in rows:
            sys.exit(f'{m.group(1)} is rated twice')
        rows[m.group(1)] = (int(m.group(2)), int(m.group(3)))
matrix = set(re.findall(r'^\| (F-\d{3}) \|', pathlib.Path('docs/research/FEATURE_MATRIX.md').read_text(encoding='utf-8'), re.M))
missing, extra = matrix - set(rows), set(rows) - matrix
if missing or extra:
    sys.exit(f'not rated: {sorted(missing)}; not in the matrix: {sorted(extra)}')
def group(fid):
    n = int(fid[2:])
    if n <= 20 or 85 <= n <= 87: return 'M3'
    if 30 <= n <= 40: return 'M4'
    if 50 <= n <= 60: return 'M5'
    return 'M6'
def score(items):
    w = sum(g for g, _ in items)
    return sum(g * share[s] for g, s in items) / w if w else 0.0
items = list(rows.values())
print(f'parity: {score(items) * 100:.1f} %')
for m in ('M3', 'M4', 'M5', 'M6'):
    part = [rows[f] for f in rows if group(f) == m]
    print(f'{m}: {score(part) * 100:.1f} % of {len(part)} rows')
verified = sum(g for g, s in items if s == 4) / sum(g for g, _ in items)
print(f'M7 (verified against the reference): {verified * 100:.1f} %')
PY
# Every matrix row has a functional test module or a named reason (M7.4, docs/research/FUNCTIONAL_COVERAGE.md).
scripts/functional.sh --check
