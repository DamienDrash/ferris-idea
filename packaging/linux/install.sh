#!/bin/sh
# Installs Ferris IDEA from the unpacked portable archive (SPEC-0354 behaviour 10).
#
#   ./install.sh                         copy bin/ and share/ into ~/.local (no root needed)
#   ./install.sh --prefix /usr/local     copy them into another folder (root for system folders)
#   ./install.sh --uninstall [--prefix DIR]
#                                        remove exactly the files an earlier install wrote
#
# The list of installed files is kept in <prefix>/share/ferris-idea/installed-files. The desktop, MIME
# and icon caches are updated when their tools are present. Nothing is downloaded, no update check
# is set up, no system setting is changed.
set -eu
here=$(cd "$(dirname "$0")" && pwd -P)
prefix="${HOME:?HOME is not set}/.local"
uninstall=0
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) [ $# -ge 2 ] || { echo "error: --prefix needs a folder" >&2; exit 2; }; prefix="$2"; shift ;;
    --uninstall) uninstall=1 ;;
    *) echo "usage: $0 [--uninstall] [--prefix DIR]" >&2; exit 2 ;;
  esac
  shift
done
case "$prefix" in
  /*) ;;
  *) prefix="$(pwd -P)/$prefix" ;;
esac
case "$prefix" in *"
"*) echo "error: the folder name contains a line break" >&2; exit 2 ;; esac
manifest="$prefix/share/ferris-idea/installed-files"

caches() {
  if command -v update-desktop-database >/dev/null 2>&1 && [ -d "$prefix/share/applications" ]; then
    update-desktop-database -q "$prefix/share/applications" 2>/dev/null || true
  fi
  if command -v update-mime-database >/dev/null 2>&1 && [ -d "$prefix/share/mime/packages" ]; then
    update-mime-database "$prefix/share/mime" >/dev/null 2>&1 || true
  fi
  if command -v gtk-update-icon-cache >/dev/null 2>&1 && [ -d "$prefix/share/icons/hicolor" ]; then
    gtk-update-icon-cache -q -t -f "$prefix/share/icons/hicolor" 2>/dev/null || true
  fi
}

if [ "$uninstall" = 1 ]; then
  [ -f "$manifest" ] || { echo "error: no Ferris IDEA install in $prefix ($manifest not found)" >&2; exit 1; }
  while IFS= read -r f; do
    case "$f" in "" | /* | *..*) continue ;; esac
    if [ -f "$prefix/$f" ] || [ -L "$prefix/$f" ]; then rm -f "$prefix/$f"; fi
  done <"$manifest"
  rm -f "$manifest"
  rmdir "$prefix/share/ferris-idea" 2>/dev/null || true
  caches
  echo "removed Ferris IDEA from $prefix"
  exit 0
fi

[ -x "$here/bin/ferris-idea" ] || { echo "error: $here/bin/ferris-idea not found; run install.sh from the unpacked archive" >&2; exit 1; }
files=$(cd "$here" && find bin share -type f | LC_ALL=C sort)
mkdir -p "$prefix/share/ferris-idea"
: >"$manifest.new"
echo "$files" | while IFS= read -r f; do
  [ -n "$f" ] || continue
  mkdir -p "$prefix/$(dirname "$f")"
  # A link at the target is replaced, never followed.
  rm -f "$prefix/$f"
  cp "$here/$f" "$prefix/$f"
  case "$f" in bin/*) chmod 755 "$prefix/$f" ;; *) chmod 644 "$prefix/$f" ;; esac
  printf '%s\n' "$f" >>"$manifest.new"
done
mv "$manifest.new" "$manifest"
caches
echo "installed Ferris IDEA into $prefix ($("$prefix/bin/ferris-idea" --version))"
case ":${PATH:-}:" in
  *":$prefix/bin:"*) ;;
  *) echo "note: $prefix/bin is not on your PATH; add it to start Ferris IDEA as 'ferris-idea'" ;;
esac
