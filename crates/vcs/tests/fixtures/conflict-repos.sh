#!/usr/bin/env bash
# Builds throwaway Git repositories that stop on real conflicts, one per case and operation, so the
# same conflicts can be opened in IDECraft and next to the reference (F-075, SPEC-0037, SPEC-0101).
#
#   crates/vcs/tests/fixtures/conflict-repos.sh <target dir> [case ...]
#
# The target must not exist or be empty. Each repository is <target>/<case>-<operation>, with
# operation merge, rebase or cherry-pick, already stopped on its conflict. The identity and all
# settings are the repository's own; the global Git configuration is neither read nor written, and
# the dates are fixed, so two runs give the same commits.
set -euo pipefail

target="${1:?usage: conflict-repos.sh <target dir> [case ...]}"
shift
all_cases=(same-lines adjacent delete-modify modify-delete add-add rename-edit whitespace crlf
  binary latin1 three-in-one two-files)
if [ "$#" -gt 0 ]; then cases=("$@"); else cases=("${all_cases[@]}"); fi

mkdir -p "$target"
if [ -n "$(ls -A "$target")" ]; then
  echo "conflict-repos.sh: $target is not empty" >&2
  exit 1
fi
target="$(cd "$target" && pwd)"

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0
export GIT_AUTHOR_DATE="2026-01-01T12:00:00Z" GIT_COMMITTER_DATE="2026-01-01T12:00:00Z"

commit() {
  git add -A
  git commit -q -m "$1"
}

# Per case: `<case>_base` writes the common ancestor, `<case>_theirs` the change on branch
# feature and `<case>_ours` the change on main.
same_lines_base() { printf 'alpha\nbeta\ngamma\ndelta\n' >greeting.txt; }
same_lines_ours() { printf 'alpha\nbeta from main\ngamma\ndelta\n' >greeting.txt; }
same_lines_theirs() { printf 'alpha\nbeta from feature\ngamma\ndelta\n' >greeting.txt; }

adjacent_base() { printf 'one\ntwo\nthree\nfour\nfive\n' >list.txt; }
adjacent_ours() { printf 'one\nTWO (main)\nthree\nfour\nfive\n' >list.txt; }
adjacent_theirs() { printf 'one\ntwo\nTHREE (feature)\nfour\nfive\n' >list.txt; }

delete_modify_base() {
  printf 'keep\nme\nor not\n' >notes.txt
  printf 'other\n' >other.txt
}
delete_modify_ours() { git rm -q notes.txt; }
delete_modify_theirs() { printf 'keep\nme, changed on feature\nor not\n' >notes.txt; }

modify_delete_base() {
  printf 'keep\nme\nor not\n' >notes.txt
  printf 'other\n' >other.txt
}
modify_delete_ours() { printf 'keep\nme, changed on main\nor not\n' >notes.txt; }
modify_delete_theirs() { git rm -q notes.txt; }

add_add_base() { printf 'readme\n' >README.txt; }
add_add_ours() { printf 'shared start\nadded on main\nshared end\n' >new.txt; }
add_add_theirs() { printf 'shared start\nadded on feature\nshared end\n' >new.txt; }

rename_edit_base() {
  printf 'line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\n' >old-name.txt
}
rename_edit_ours() {
  git mv old-name.txt new-name.txt
  printf 'line 1\nline 2 (main)\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\n' >new-name.txt
}
rename_edit_theirs() {
  printf 'line 1\nline 2 (feature)\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\n' >old-name.txt
}

whitespace_base() { printf 'fn main() {\n    let total = 1;\n    print(total);\n}\n' >code.rs; }
whitespace_ours() { printf 'fn main() {\n\tlet total = 1;\n    print(total);\n}\n' >code.rs; }
whitespace_theirs() { printf 'fn main() {\n    let total = 2;\n    print(total);\n}\n' >code.rs; }

crlf_base() { printf 'first\r\nsecond\r\nthird\r\n' >windows.txt; }
crlf_ours() { printf 'first\r\nsecond from main\r\nthird\r\n' >windows.txt; }
crlf_theirs() { printf 'first\r\nsecond from feature\r\nthird\r\n' >windows.txt; }

binary_base() {
  printf 'BIN\000\001\002base\n' >blob.bin
  printf 'text\n' >text.txt
}
binary_ours() { printf 'BIN\000\001\002main\n' >blob.bin; }
binary_theirs() { printf 'BIN\000\001\002feature\n' >blob.bin; }

# Not UTF-8: `gr\374n` is "grün" in ISO 8859-1; the conflict is in the line after it.
latin1_base() { printf 'gr\374n\nzwei\ndrei\n' >umlaut.txt; }
latin1_ours() { printf 'gr\374n\nzwei (main)\ndrei\n' >umlaut.txt; }
latin1_theirs() { printf 'gr\374n\nzwei (feature)\ndrei\n' >umlaut.txt; }

# Three conflicts: the first and the third are simple (different words of one line), the second
# is a real one (the same word changed differently).
three_in_one_base() {
  printf '%s\n' 'let width = 10, height = 20;' 'stable a' 'stable b' 'let colour = red;' \
    'stable c' 'stable d' 'fn area(left, right) {}' 'end' >shapes.txt
}
three_in_one_ours() {
  printf '%s\n' 'let width = 11, height = 20;' 'stable a' 'stable b' 'let colour = green;' \
    'stable c' 'stable d' 'fn area(top, right) {}' 'end' >shapes.txt
}
three_in_one_theirs() {
  printf '%s\n' 'let width = 10, height = 25;' 'stable a' 'stable b' 'let colour = blue;' \
    'stable c' 'stable d' 'fn area(left, bottom) {}' 'end' >shapes.txt
}

# Two conflicted files, to see the file list and that the operation waits for both.
two_files_base() {
  printf 'a1\na2\na3\n' >a.txt
  printf 'b1\nb2\nb3\n' >b.txt
}
two_files_ours() {
  printf 'a1\na2 main\na3\n' >a.txt
  printf 'b1\nb2 main\nb3\n' >b.txt
}
two_files_theirs() {
  printf 'a1\na2 feature\na3\n' >a.txt
  printf 'b1\nb2 feature\nb3\n' >b.txt
}

build() {
  local case="$1" op="$2" fn="${1//-/_}" dir="$target/$1-$2"
  git init -q -b main "$dir"
  cd "$dir"
  git config user.name "Conflict Fixture"
  git config user.email "fixture@example.invalid"
  git config core.autocrlf false
  git config commit.gpgsign false
  git config merge.conflictStyle merge
  "${fn}_base"
  commit "base"
  git checkout -q -b feature
  "${fn}_theirs"
  commit "feature: $case"
  git checkout -q main
  "${fn}_ours"
  commit "main: $case"
  case "$op" in
    merge) git merge --no-edit feature >/dev/null 2>&1 || true ;;
    cherry-pick) git cherry-pick feature >/dev/null 2>&1 || true ;;
    rebase)
      git checkout -q feature
      git rebase main >/dev/null 2>&1 || true
      ;;
  esac
  if [ -z "$(git ls-files -u)" ]; then
    echo "conflict-repos.sh: $case-$op did not stop on a conflict" >&2
    exit 1
  fi
  cd "$target"
}

for case in "${cases[@]}"; do
  for op in merge rebase cherry-pick; do
    build "$case" "$op"
  done
done
echo "$target"
