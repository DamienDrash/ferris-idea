#!/bin/sh
# Records terminal output of installed programs once, as fixtures (SPEC-0038 AK-7, SPEC-0062 AK-8).
# $1 = output folder, $2 = repository. macOS `script` puts four bytes (`^D` and two backspaces) in
# front of each recording; the fixtures are the recordings without them, and `xterm-256color.keys`
# is the key capabilities (k*, smkx, rmkx, u6-u9) of infocmp.txt.
set -eu
out="$1"
repo="$2"
export TERM=xterm-256color LC_ALL=en_US.UTF-8
mkdir -p "$out/d/folder"
touch "$out/d/plain.txt" "$out/d/run.sh"
chmod +x "$out/d/run.sh"
ln -sf plain.txt "$out/d/link"

cat >"$out/tput.sh" <<'EOF'
tput smkx; tput smcup; tput clear
tput cup 2 4; tput setaf 1; tput bold; printf red; tput sgr0
tput cup 0 0; tput smacs; printf 'lqqk'; tput rmacs
tput cup 1 0; tput smacs; printf 'x'; tput rmacs; printf '  '; tput smacs; printf 'x'; tput rmacs
tput cup 4 0; tput setab 4; tput el; printf 'bar'; tput sgr0
tput cup 5 0; printf 'abcdef'; tput cub 4; tput dch 2; tput ich 1; printf 'X'
tput cup 6 0; tput hts; tput cup 6 3; tput hts; tput cup 6 0; printf '\tT'; tput tbc
tput cup 7 0; tput sc; tput cup 9 9; printf 'far'; tput rc; printf 'back'
tput cup 8 0; tput smir; printf 'world'; tput cup 8 0; printf 'hello '; tput rmir
tput csr 2 4; tput cup 4 0; printf '\n'; tput csr 0 23; tput cup 11 0; printf 'end'
tput u7
EOF
printf 'tput smkx; tput smcup; tput clear; tput cup 1 1; printf alt; tput rmcup; tput rmkx; printf after' >"$out/alt.sh"

script -q "$out/tput.raw" sh "$out/tput.sh" >/dev/null
script -q "$out/alt.raw" sh "$out/alt.sh" >/dev/null
script -q "$out/ls.raw" sh -c "cd '$out/d' && CLICOLOR_FORCE=1 ls -G -1" >/dev/null
script -q "$out/git.raw" git -C "$repo" --no-pager log --color=always --oneline --no-decorate -3 9d769e4d >/dev/null
infocmp -1 xterm-256color >"$out/infocmp.txt"
for f in "$out"/*.raw; do
  echo "== $f $(wc -c <"$f")"
  cat -v "$f"
  echo
done
wc -l "$out/infocmp.txt"
