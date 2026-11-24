#!/bin/sh
# Functional tests per feature-matrix row (ROADMAP M7.4): runs tests/functional and writes the
# coverage table docs/research/FUNCTIONAL_COVERAGE.md (F-ID → tests → result).
#
#   functional.sh           run the suite (reference databases when they run) and write the table
#   functional.sh --check   only check that every matrix row has a module (no build; used by parity.sh)
set -eu
# Variables under the former name (IDECRAFT_*) still count while today's is not set. rebrand:keep
for former in $(env | sed -n 's/^IDECRAFT_\([A-Za-z0-9_]*\)=.*/\1/p'); do # rebrand:keep
  eval "[ -n \"\${FERRIS_IDEA_$former+x}\" ] || { FERRIS_IDEA_$former=\$IDECRAFT_$former; export FERRIS_IDEA_$former; }" # rebrand:keep
done
cd "$(dirname "$0")/.."

if [ "${1:-}" != "--check" ]; then
  # The same reference databases as scripts/ci-local.sh, when they run.
  nc -z 127.0.0.1 54329 2>/dev/null && export FERRIS_IDEA_TEST_PG="${FERRIS_IDEA_TEST_PG:-postgresql://idecraft@127.0.0.1:54329/inventory?sslmode=disable}"
  nc -z 127.0.0.1 33069 2>/dev/null && export FERRIS_IDEA_TEST_MYSQL="${FERRIS_IDEA_TEST_MYSQL:-mysql://idecraft@127.0.0.1:33069/inventory?ssl-mode=DISABLED}"
  nc -z 127.0.0.1 33079 2>/dev/null && export FERRIS_IDEA_TEST_MARIADB="${FERRIS_IDEA_TEST_MARIADB:-mariadb://idecraft@127.0.0.1:33079/inventory?ssl-mode=DISABLED}"
  # Their password files (db.sh secure). The harness copies them into each test project's password
  # folder for the MCP server (FERRIS_IDEA_TEST_SECRETS_DIR, SPEC-0008 behaviour 14), never the keychain.
  # secret_file VAR FILE: export VAR naming FILE when FILE exists and VAR is not set yet.
  secret_file() { if [ -s "$2" ] && [ -z "$(printenv "$1")" ]; then export "$1=$2"; fi; }
  secrets_dir="${XDG_CONFIG_HOME:-$HOME/.config}/ferris-idea"
  [ -d "$secrets_dir" ] || secrets_dir="${XDG_CONFIG_HOME:-$HOME/.config}/idecraft" # rebrand:keep (the settings folder before its takeover, SPEC-0356)
  secret_file FERRIS_IDEA_TEST_PG_PASSWORD_FILE "$secrets_dir/refdb-postgres.pass"
  secret_file FERRIS_IDEA_TEST_MYSQL_PASSWORD_FILE "$secrets_dir/refdb-mysql.pass"
  secret_file FERRIS_IDEA_TEST_MARIADB_PASSWORD_FILE "$secrets_dir/refdb-mysql.pass"
  report=$(mktemp -d)
  trap 'rm -rf "$report"' EXIT
  status=0
  FERRIS_IDEA_FUNCTIONAL_REPORT="$report" cargo test -p ferris-idea --test functional >"$report/out.txt" 2>&1 || status=$?
  export FUNCTIONAL_REPORT="$report"
fi

python3 -I - "${1:-}" <<'PY'
import os, pathlib, re, subprocess, sys, datetime
check = sys.argv[1] == '--check'
matrix = {}
for line in pathlib.Path('docs/research/FEATURE_MATRIX.md').read_text(encoding='utf-8').splitlines():
    m = re.match(r'^\| (F-\d{3}) \| ([^|]+) \|', line)
    if m:
        matrix[m.group(1)] = m.group(2).strip()
modules = {}
for f in sorted(pathlib.Path('tests/functional').glob('f[0-9][0-9][0-9]_*.rs')):
    fid = 'F-' + f.name[1:4]
    text = f.read_text(encoding='utf-8')
    head = '\n'.join(l[3:].strip() for l in text.splitlines() if l.startswith('//!'))
    selftest = re.search(r'self-test: ([^\n]+)', head)
    reason = re.search(r'not-tested: (.+?)(?:\n\n|\Z)', head, re.S)
    modules[fid] = {
        'module': f.stem,
        'tests': re.findall(r'#\[test\]\s*\n\s*fn (\w+)', text),
        'selftest': selftest.group(1).strip() if selftest else '',
        'reason': ' '.join(reason.group(1).split()) if reason else '',
    }
missing = sorted(set(matrix) - set(modules))
extra = sorted(set(modules) - set(matrix))
untested = sorted(f for f, m in modules.items() if not m['tests'] and not m['reason'])
if missing or extra or untested:
    sys.exit(f'functional tests: no module for {missing}; not in the matrix: {extra}; no test and no reason: {untested}')
with_tests = sum(1 for m in modules.values() if m['tests'])
print(f'functional tests: {with_tests} of {len(matrix)} rows tested, {len(matrix) - with_tests} with a named reason')
if check:
    sys.exit(0)

report = pathlib.Path(os.environ['FUNCTIONAL_REPORT'])
out = (report / 'out.txt').read_text(encoding='utf-8', errors='replace')
results = {}
for m in re.finditer(r'^test (f\d{3}_\w+)::(\w+) \.\.\. (ok|FAILED|ignored)', out, re.M):
    results[(m.group(1), m.group(2))] = m.group(3)
skips = {}
skipfile = report / 'skipped.tsv'
if skipfile.exists():
    for line in skipfile.read_text(encoding='utf-8').splitlines():
        name, _, why = line.partition('\t')
        mod, _, test = name.partition('::')
        skips[(mod, test)] = why
totals = {'passed': 0, 'skipped': 0, 'failed': 0}
rows = []
for fid, title in matrix.items():
    m = modules[fid]
    cells, states = [], []
    for t in m['tests']:
        r = results.get((m['module'], t))
        if r is None:
            state = 'not run'
        elif r == 'ok' and (m['module'], t) in skips:
            state = 'skipped: ' + skips[(m['module'], t)].replace('|', '/')[:160]
        else:
            state = {'ok': 'passed', 'FAILED': 'failed', 'ignored': 'ignored'}[r]
        key = state.split(':')[0]
        if key in totals:
            totals[key] += 1
        cells.append(f'`{t}`')
        states.append(state)
    if m['tests']:
        passed = states.count('passed')
        summary = f'{passed}/{len(states)} passed'
        others = [s for s in states if s != 'passed']
        if others:
            summary += '; ' + '; '.join(sorted(set(others)))
    else:
        summary = 'not tested: ' + m['reason']
    window = m['selftest'] or '—'
    rows.append(f'| {fid} | {title} | `{m["module"]}`: ' + (', '.join(cells) or '—') + f' | {summary} | {window} |')
commit = subprocess.run(['git', 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True).stdout.strip()
date = datetime.date.today().isoformat()
doc = f'''# Funktionstests je Matrixzeile

**Stand:** {date} · Commit `{commit}` · erzeugt von `scripts/functional.sh` · Grundlage: [Feature-Matrix](FEATURE_MATRIX.md), Tests: `tests/functional/`

Je Zeile der Feature-Matrix ein Testmodul, geschrieben aus Zeile und Akzeptanzkriterien der Spezifikation,
nicht aus der Implementierung. Die Tests bedienen Ferris IDEA nur von außen: Befehlszeile `ferris-idea`, MCP über
stdio und über den HTTP-Transport des Fensters, auf Kopien von `tests/reference-projects` und gegen die
Referenzdatenbanken. Verhalten, das nur ein Fenster zeigt, prüft der Fenster-Selbsttest
(`ferris-idea ui --selftest`, `scripts/ci-window.sh`); die Spalte nennt seine Schritte.

**Ergebnis:** {len(matrix)} Zeilen, {with_tests} mit Tests, {len(matrix) - with_tests} mit benanntem Grund ·
{sum(totals.values())} Tests: {totals["passed"]} bestanden, {totals["skipped"]} übersprungen (Werkzeug oder Datenbank fehlt), {totals["failed"]} fehlgeschlagen.

| ID | Funktion | Tests | Ergebnis | Fenster-Selbsttest |
|---|---|---|---|---|
''' + '\n'.join(rows) + '\n'
pathlib.Path('docs/research/FUNCTIONAL_COVERAGE.md').write_text(doc, encoding='utf-8')
print(f'{totals["passed"]} passed, {totals["skipped"]} skipped, {totals["failed"]} failed → docs/research/FUNCTIONAL_COVERAGE.md')
PY
[ "${1:-}" = "--check" ] && exit 0
if [ "$status" -ne 0 ]; then
  grep -E '^(test .* FAILED|---- |thread )' "$FUNCTIONAL_REPORT/out.txt" | head -40
  exit "$status"
fi
