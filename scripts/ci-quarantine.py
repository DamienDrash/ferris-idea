#!/usr/bin/env python3
"""Reads scripts/ci-quarantine.txt (ADR-0030, decision 11).

    ci-quarantine.py check [LIST]    exit 1 with the faulty lines when an entry lacks a field
    ci-quarantine.py filter [LIST]   print the entries as one nextest filter, `(a) | (b)`, or nothing
    ci-quarantine.py count [LIST]    print the number of entries

LIST defaults to ci-quarantine.txt beside this script (the build server passes the list of the
tree under test to its own installed copy of this script).

Format of an entry: four tab-separated fields, a nextest filter, the reason, the date it entered
(YYYY-MM-DD) and the issue that owns the fix (#N). Lines starting with # and empty lines are left out.
"""
import pathlib
import re
import sys

PATH = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else pathlib.Path(__file__).resolve().parent / 'ci-quarantine.txt'


def entries():
    good, bad = [], []
    if not PATH.exists():
        return good, bad
    for n, line in enumerate(PATH.read_text(encoding='utf-8').splitlines(), 1):
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        fields = [f.strip() for f in line.split('\t')]
        problems = []
        if len(fields) != 4:
            problems.append(f'{len(fields)} tab-separated fields instead of 4')
        else:
            filt, reason, date, issue = fields
            if not filt:
                problems.append('no filter')
            if len(reason) < 10:
                problems.append('no reason (at least a few words)')
            if not re.fullmatch(r'\d{4}-\d{2}-\d{2}', date):
                problems.append('date is not YYYY-MM-DD')
            if not re.fullmatch(r'#\d+', issue):
                problems.append('owner issue is not #N')
            if "'" in filt or '"' in filt or '`' in filt or '$' in filt:
                problems.append('filter has a quote, backquote or $')
        if problems:
            bad.append(f'{PATH.name}:{n}: ' + '; '.join(problems))
        else:
            good.append(fields[0])
    return good, bad


def main():
    what = sys.argv[1] if len(sys.argv) > 1 else ''
    good, bad = entries()
    if what == 'check':
        for b in bad:
            print(b, file=sys.stderr)
        return 1 if bad else 0
    if what == 'filter':
        if bad:
            for b in bad:
                print(b, file=sys.stderr)
            return 1
        print(' | '.join(f'({g})' for g in good))
        return 0
    if what == 'count':
        print(len(good))
        return 1 if bad else 0
    print(__doc__, file=sys.stderr)
    return 2


sys.exit(main())
