#!/bin/sh
# Writes THIRD-PARTY-NOTICES.md: every third-party Rust package of the workspace (Cargo.lock, all
# platforms and features) with its licence, and the licence and notice files the packages ship,
# read from the local Cargo registry (`cargo fetch` puts them there). Native libraries that are
# compiled in from vendored sources are named in a hand-kept table below, with the licence files
# of those sources.
#
# The output depends only on Cargo.lock and on this script: packages and texts are sorted, equal
# texts are printed once, and the file carries no date and no local path. Run it after every
# change of Cargo.lock and before a release (LICENSING.md, ADR-0032).
#
#   third-party-notices.sh           write THIRD-PARTY-NOTICES.md
#   third-party-notices.sh --check   fail if the committed file differs from a fresh run
set -eu
cd "$(dirname "$0")/.."
python3 -I - "${1:-}" <<'PY'
import hashlib, json, pathlib, re, subprocess, sys

check = sys.argv[1] == '--check'
if sys.argv[1] not in ('', '--check'):
    sys.exit('usage: scripts/third-party-notices.sh [--check]')
OUT = pathlib.Path('THIRD-PARTY-NOTICES.md')

# Native code that crate metadata does not describe: the package that vendors it, what it is, its
# licence, a note, and the licence files inside the package folder. Kept by hand; a path that no
# longer exists stops the run, so an update of the package is noticed.
NATIVE = [
    ('libgit2-sys', 'libgit2', 'GPL-2.0-only with the linking exception printed at the top of its COPYING',
     'Reached through the UI toolkit (`gpui_util` → `git2`). The exception in its `COPYING` permits '
     'linking the library into a program under any licence. `COPYING` also carries the notices of the '
     'code bundled with libgit2.',
     ['libgit2/COPYING']),
    ('libz-sys', 'zlib', 'Zlib', 'Built for libgit2 where the system has no zlib.', ['src/zlib/LICENSE']),
    ('openssl-src', 'OpenSSL', 'Apache-2.0',
     'Vendored build for the HTTP client on Android and for `ferris-idea serve` on Linux.',
     ['openssl/LICENSE.txt']),
    ('libduckdb-sys', 'DuckDB', 'MIT',
     'Bundled build of the DuckDB library. DuckDB bundles further third-party code of its own; its '
     'source distribution lists it.',
     ['LICENSE']),
    ('libsqlite3-sys', 'SQLite', 'Public domain (blessing instead of a licence)',
     'Bundled build of the SQLite amalgamation.', []),
    ('freetype-sys', 'FreeType', 'FTL OR GPL-2.0-only; used under the FreeType License (FTL)',
     'Linux builds only. The FTL asks for this credit: portions of this software are copyright © The '
     'FreeType Project (www.freetype.org). All rights reserved.',
     ['freetype2/LICENSE.TXT', 'freetype2/docs/FTL.TXT']),
    ('ring', 'BoringSSL and fiat-crypto code inside ring', 'Apache-2.0 AND ISC (and the notices in its files)',
     'TLS and cryptography.', ['LICENSE', 'LICENSE-BoringSSL', 'LICENSE-other-bits', 'third_party/fiat/LICENSE']),
    ('tree-sitter', 'tree-sitter runtime with ICU Unicode headers', 'MIT; Unicode data files under the Unicode licence',
     'The parser runtime. The grammars (`tree-sitter-*`) are generated C code in their packages, listed '
     'in the table of packages.',
     ['LICENSE', 'src/unicode/LICENSE']),
    ('odpic-sys', 'ODPI-C', 'UPL-1.0 OR Apache-2.0',
     'Oracle driver interface. The Oracle Instant Client it loads at run time is proprietary, installed '
     'by the user and never shipped.',
     ['odpi/LICENSE.txt', 'odpi/NOTICE.txt']),
    ('mongocrypt-sys', 'libmongocrypt (bindings)', 'Apache-2.0',
     'Bindings only; the package ships no licence file of its own.', []),
]
# Licence or notice files in the top folder of a package.
FILE = re.compile(r'^(licen[sc]e|copying|notice|unlicense|copyright|patents)', re.I)
# Standard text printed for packages that ship no licence file.
MIT_TEMPLATE = '''MIT License

Copyright (c) <copyright holders>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.'''

meta = json.loads(subprocess.run(
    ['cargo', 'metadata', '--format-version', '1', '--locked', '--all-features'],
    capture_output=True, text=True, check=True).stdout)
# Third-party packages kept in `vendor/` with changes of Ferris IDEA (each folder's FERRIS_IDEA.md names
# them) count like registry packages; the table says where the changes are written down.
ROOT = pathlib.Path.cwd().resolve()
def vendored(p):
    d = pathlib.Path(p['manifest_path']).parent.resolve()
    return d.parent == ROOT / 'vendor' and (d / 'FERRIS_IDEA.md').is_file()
packages = sorted((p for p in meta['packages'] if p['source'] or vendored(p)),
                  key=lambda p: (p['name'], p['version']))

def normal(raw):
    text = raw.decode('utf-8', errors='replace').replace('\r\n', '\n').replace('\r', '\n')
    lines = [line.rstrip() for line in text.split('\n')]
    return '\n'.join(lines).strip('\n')

texts = {}    # digest -> text
users = {}    # digest -> ['name version (file)']
def add(text, user):
    digest = hashlib.sha256(text.encode('utf-8')).hexdigest()
    texts[digest] = text
    users.setdefault(digest, []).append(user)
    return digest

def folder(p):
    d = pathlib.Path(p['manifest_path']).parent
    if not d.is_dir():
        sys.exit(f'error: sources of {p["name"]} {p["version"]} are not in the registry (cargo fetch)')
    return d

own = {}      # (name, version) -> [digest]
for p in packages:
    d = folder(p)
    names = {f.name for f in d.iterdir() if f.is_file() and FILE.match(f.name)}
    if p.get('license_file'):
        names.add(p['license_file'])
    found = []
    for name in sorted(names):
        f = d / name
        if f.is_file():
            found.append(add(normal(f.read_bytes()), f'{p["name"]} {p["version"]} (`{name}`)'))
    own[(p['name'], p['version'])] = found

native_rows = []
for crate, what, licence, note, files in NATIVE:
    for p in (p for p in packages if p['name'] == crate):
        d = folder(p)
        found = []
        for rel in files:
            f = d / rel
            if not f.is_file():
                sys.exit(f'error: {crate} {p["version"]} has no {rel}; update NATIVE in this script')
            found.append(add(normal(f.read_bytes()), f'{what}, in {crate} {p["version"]} (`{rel}`)'))
        native_rows.append((what, crate, p['version'], licence, note, found))

def ids(expression):
    return [part.strip() for part in re.split(r'\s+OR\s+|/', expression or '') if part.strip()]

# Packages without a licence file get a standard text: MIT or Apache-2.0 where the package offers
# one of them, else the text another package under the same single licence ships.
apache = add(normal(pathlib.Path('LICENSES/Apache-2.0.txt').read_bytes()),
             'standard text of the Apache License 2.0 (`LICENSES/Apache-2.0.txt`)')
mit = add(MIT_TEMPLATE, 'standard text of the MIT License')
standard = {}
bare = []     # (package, chosen id)
for p in packages:
    if own[(p['name'], p['version'])]:
        continue
    offered = ids(p['license'])
    chosen = next((i for i in ('MIT', 'Apache-2.0') if i in offered), offered[0] if len(offered) == 1 else None)
    if chosen is None:
        sys.exit(f'error: {p["name"]} {p["version"]} ships no licence file and its licence '
                 f'"{p["license"]}" has no standard text here')
    bare.append((p, chosen))
for p, chosen in bare:
    if chosen in standard:
        continue
    if chosen == 'MIT':
        standard[chosen] = mit
    elif chosen == 'Apache-2.0':
        standard[chosen] = apache
    else:
        donor = next((q for q in packages if q['license'] == chosen and len(own[(q['name'], q['version'])]) == 1), None)
        if donor is None:
            sys.exit(f'error: no package ships the text of {chosen} (needed for {p["name"]})')
        standard[chosen] = own[(donor['name'], donor['version'])][0]
for p, chosen in bare:
    users[standard[chosen]].append(f'{p["name"]} {p["version"]} (no licence file in the package)')

# Number the texts in the order of their first user.
order = sorted(texts, key=lambda digest: (sorted(users[digest])[0].lower(), digest))
label = {digest: f'T{n:04d}' for n, digest in enumerate(order, 1)}

def cell(s):
    return s.replace('|', '\\|').replace('\n', ' ')
def source(p):
    return f'<https://crates.io/crates/{p["name"]}/{p["version"]}>'

out = []
w = out.append
w('# Third-party notices')
w('')
w('<!-- Generated by scripts/third-party-notices.sh from Cargo.lock. Do not edit by hand. -->')
w('')
w('Ferris IDEA is built from the third-party software below. Each part stays under its own licence;')
w('Ferris IDEA\'s own licence ([`LICENSE.md`](LICENSE.md)) does not apply to it. Material kept in this')
w('repository itself (keymaps, themes, icons, font, the built-in English dictionary) is named in')
w('[`NOTICE`](NOTICE) and [`LICENSES/`](LICENSES/README.md).')
w('')
w(f'The list has every package in `Cargo.lock`, for all platforms and features: {len(packages)} packages.')
w('A given binary contains only part of them; build tools, test helpers and packages of other')
w('platforms are listed too. The fuzzing workspace (`fuzz/`) is not shipped and not listed.')
w('')
w('## 1. Native libraries compiled into the binaries')
w('')
w('| Library | Through package | Licence | Note | Texts |')
w('|---|---|---|---|---|')
for what, crate, version, licence, note, found in native_rows:
    refs = ', '.join(label[d] for d in found) or '—'
    w(f'| {cell(what)} | `{crate}` {version} | {cell(licence)} | {cell(note)} | {refs} |')
w('')
w('## 2. Rust packages')
w('')
w('The licence is the expression the package declares. Where it offers a choice (`OR`, `/`), Ferris IDEA')
w('uses the package under a permissive option of that choice. "Texts" points to section 4.')
w('')
w('| Package | Version | Licence | Texts |')
w('|---|---|---|---|')
chosen_of = {(p['name'], p['version']): chosen for p, chosen in bare}
for p in packages:
    key = (p['name'], p['version'])
    found = own[key] or [standard[chosen_of[key]]]
    refs = ', '.join(label[d] for d in found)
    licence = cell(p["license"] or "see its licence file")
    if vendored(p):
        licence += f'; kept in `vendor/{pathlib.Path(p["manifest_path"]).parent.name}/` with changes of Ferris IDEA, named in its `FERRIS_IDEA.md`'
    w(f'| [{p["name"]}](https://crates.io/crates/{p["name"]}/{p["version"]}) | {p["version"]} | {licence} | {refs} |')
w('')
w('## 3. Packages under the Mozilla Public License 2.0')
w('')
w('These packages are used unchanged. Their source code, in the form the licence asks for, is')
w('available from the crates.io registry at the address given and from the repository the package names.')
w('')
for p in packages:
    if 'MPL-2.0' in ids(p['license']) and len(ids(p['license'])) == 1:
        repo = f', {p["repository"]}' if p.get('repository') else ''
        w(f'- {p["name"]} {p["version"]}: {source(p)}{repo}')
w('')
w('## 4. Licence texts and notices')
w('')
w('Each text is printed once, as shipped in the package named, with line ends and trailing blanks')
w('normalised. A package that ships no licence file is listed under the standard text of its licence;')
w('its copyright holders are the authors its manifest names:')
w('')
for p, chosen in bare:
    authors = ', '.join(re.sub(r'\s*<[^>]*>', '', a).strip() for a in p.get('authors') or []) or f'the {p["name"]} authors'
    w(f'- {p["name"]} {p["version"]} ({chosen}): {cell(authors)}')
w('')
for digest in order:
    text = texts[digest]
    longest = max((len(m) for m in re.findall(r'`+', text)), default=0)
    fence = '`' * max(3, longest + 1)
    w(f'### {label[digest]}')
    w('')
    w('Used by: ' + '; '.join(sorted(users[digest], key=str.lower)) + '.')
    w('')
    w(fence + 'text')
    w(text)
    w(fence)
    w('')
result = '\n'.join(out).rstrip('\n') + '\n'
if check:
    if not OUT.is_file() or OUT.read_text(encoding='utf-8') != result:
        sys.exit('THIRD-PARTY-NOTICES.md is out of date: run scripts/third-party-notices.sh')
    print(f'THIRD-PARTY-NOTICES.md is current ({len(packages)} packages, {len(order)} texts)')
else:
    OUT.write_text(result, encoding='utf-8')
    print(f'wrote THIRD-PARTY-NOTICES.md: {len(packages)} packages, {len(order)} texts, {len(result.encode("utf-8"))} bytes')
PY
