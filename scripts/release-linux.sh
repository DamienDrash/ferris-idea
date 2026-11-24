#!/bin/sh
# Builds the Linux packages of Ferris IDEA (SPEC-0354): release build, then a portable archive
# (.tar.gz), an AppImage and a Debian package (.deb) for the machine's architecture (x86_64 or
# aarch64), their checks, a SHA-256 checksum list and a detached signature of that list.
#
# It uploads nothing and publishes nothing: publishing the packages is a separate step that needs
# the project owner's approval (GO-4). It talks only to the pinned tools' release places and to
# crates.io (behaviour 4).
#
#   release-linux.sh --out DIR [--arch x86_64|aarch64] [--unsigned]
#                                 build, package, check, write checksums, sign
#   release-linux.sh --out DIR --check [--unsigned]
#                                 only the preconditions: Linux, architecture, clean tree, key, tools
#   release-linux.sh --sign DIR   sign a finished --unsigned build in DIR without rebuilding it
#                                 (also on macOS): check the files against the -unsigned checksum
#                                 list, drop -unsigned from their names, write and sign a new list
#
# The signing key is the project owner's and is read from the environment at run time, never
# stored or printed. Exactly one of:
#   FERRIS_IDEA_LINUX_MINISIGN_KEY      path of a minisign secret key file (mode 600), together with
#   FERRIS_IDEA_LINUX_MINISIGN_PUBKEY   path of its public key file, used to check the signature
#   FERRIS_IDEA_LINUX_GPG_KEY           ID or fingerprint of an OpenPGP secret key in the keyring
# minisign or gpg ask for the passphrase themselves. Without a key the script refuses to build;
# `--unsigned` builds without one and puts `-unsigned` into every file name.
#
# Other settings: FERRIS_IDEA_RELEASE_TOOLS (folder for the pinned tools, default
# <target>/release-tools), FERRIS_IDEA_DEB_MAINTAINER (the Debian Maintainer field), SOURCE_DATE_EPOCH
# (default: the commit's time), CARGO_TARGET_DIR.
set -eu
# Variables under the former name (IDECRAFT_*) still count while today's is not set. rebrand:keep
for former in $(env | sed -n 's/^IDECRAFT_\([A-Za-z0-9_]*\)=.*/\1/p'); do # rebrand:keep
  eval "[ -n \"\${FERRIS_IDEA_$former+x}\" ] || { FERRIS_IDEA_$former=\$IDECRAFT_$former; export FERRIS_IDEA_$former; }" # rebrand:keep
done
export FERRIS_IDEA_NO_TAKEOVER=1 # the programs this script starts never move the user's settings folder (SPEC-0356)
umask 022
cd "$(dirname "$0")/.."
src=$(pwd -P)

# Pinned tools (behaviour 4): version and SHA-256 of each download, from the projects' releases.
APPIMAGETOOL_VERSION=1.9.1
RUNTIME_RELEASE=20251108
pinned_sums() { # sets tool_sum and runtime_sum for the architecture $1
  case "$1" in
    x86_64)
      tool_sum=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
      runtime_sum=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d ;;
    aarch64)
      tool_sum=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
      runtime_sum=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444 ;;
  esac
}
RESVG_VERSION=0.45.1
HOMEPAGE=https://github.com/DamienDrash/ferris-idea

usage() {
  echo "usage: $0 --out DIR [--arch x86_64|aarch64] [--unsigned] [--check] | $0 --sign DIR" >&2
  exit 2
}
fail() { echo "error: $*" >&2; exit 1; }
step() { printf '\n== %s\n' "$*"; }

out=""; arch=""; unsigned=0; check_only=0; sign_dir=""
while [ $# -gt 0 ]; do
  case "$1" in
    --out) [ $# -ge 2 ] || usage; out="$2"; shift ;;
    --arch) [ $# -ge 2 ] || usage; arch="$2"; shift ;;
    --unsigned) unsigned=1 ;;
    --check) check_only=1 ;;
    --sign) [ $# -ge 2 ] || usage; sign_dir="$2"; shift ;;
    *) usage ;;
  esac
  shift
done

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  else shasum -a 256 "$1" | awk '{print $1}'; fi
}
check_list() { # check_list <checksum list> (in the current folder)
  if command -v sha256sum >/dev/null 2>&1; then sha256sum --quiet --strict -c "$1"
  else shasum -a 256 --quiet --strict -c "$1"; fi
}
file_mode() { stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1"; }
file_size() { wc -c <"$1" | tr -d ' '; }

# The signing key (behaviour 3): exactly one kind, checked before anything is built.
choose_key() {
  mkey="${FERRIS_IDEA_LINUX_MINISIGN_KEY:-}"
  mpub="${FERRIS_IDEA_LINUX_MINISIGN_PUBKEY:-}"
  gkey="${FERRIS_IDEA_LINUX_GPG_KEY:-}"
  if [ -n "$mkey" ] && [ -n "$gkey" ]; then
    fail "set either FERRIS_IDEA_LINUX_MINISIGN_KEY or FERRIS_IDEA_LINUX_GPG_KEY, not both"
  elif [ -n "$mkey" ]; then
    [ -f "$mkey" ] || fail "FERRIS_IDEA_LINUX_MINISIGN_KEY does not name a file"
    mode=$(file_mode "$mkey")
    case "$mode" in ?00) ;; *) fail "the minisign secret key file is readable by others (mode $mode); chmod 600 it" ;; esac
    { [ -n "$mpub" ] && [ -f "$mpub" ]; } ||
      fail "FERRIS_IDEA_LINUX_MINISIGN_PUBKEY must name the public key file of that key (the signature is checked with it)"
    command -v minisign >/dev/null 2>&1 || fail "minisign not found (Debian/Ubuntu: apt install minisign)"
    sig_kind=minisign; sig_ext=minisig
  elif [ -n "$gkey" ]; then
    command -v gpg >/dev/null 2>&1 || fail "gpg not found"
    gpg --batch --list-secret-keys -- "$gkey" >/dev/null 2>&1 ||
      fail "no OpenPGP secret key in the keyring matches FERRIS_IDEA_LINUX_GPG_KEY"
    sig_kind=gpg; sig_ext=asc
  else
    fail "no signing key: set FERRIS_IDEA_LINUX_MINISIGN_KEY (with FERRIS_IDEA_LINUX_MINISIGN_PUBKEY) or FERRIS_IDEA_LINUX_GPG_KEY, or build with --unsigned"
  fi
}

# Signs a checksum list detached and checks the signature (behaviour 14).
sign_list() { # sign_list <list> <trusted comment>
  rm -f "$1.$sig_ext"
  case "$sig_kind" in
    minisign)
      minisign -S -s "$mkey" -m "$1" -x "$1.minisig" -t "$2" >/dev/null || fail "minisign could not sign $(basename "$1")"
      minisign -V -q -p "$mpub" -m "$1" -x "$1.minisig" >/dev/null ||
        fail "the signature does not check with FERRIS_IDEA_LINUX_MINISIGN_PUBKEY (key pair mismatch?)"
      ;;
    gpg)
      gpg --local-user "$gkey" --armor --detach-sign --output "$1.asc" -- "$1" 2>"$1.gpg.log" || {
        sed -E 's/[0-9A-Fa-f]{16,40}/<key>/g' "$1.gpg.log" >&2; rm -f "$1.gpg.log"; fail "gpg could not sign $(basename "$1")"
      }
      rm -f "$1.gpg.log"
      gpg --verify -- "$1.asc" "$1" >/dev/null 2>&1 || fail "gpg --verify rejects the new signature"
      ;;
  esac
}

# --sign DIR (behaviour 16): sign an --unsigned build later, on a machine that holds the key.
if [ -n "$sign_dir" ]; then
  if [ -n "$out$arch" ] || [ "$unsigned$check_only" != 00 ]; then usage; fi
  [ -d "$sign_dir" ] || fail "$sign_dir is not a folder"
  choose_key
  cd "$sign_dir"
  found=0
  for list in ferris-idea-*-linux-*-unsigned.sha256; do
    [ -f "$list" ] || continue
    found=1
    check_list "$list" || fail "$list: a file is missing or differs from its checksum"
    signed_list="${list%-unsigned.sha256}.sha256"
    [ ! -e "$signed_list" ] || fail "$signed_list exists already"
    awk '{print $2}' "$list" | while IFS= read -r f; do
      case "$f" in *-unsigned*) ;; *) fail "$f in $list has no -unsigned in its name" ;; esac
      new=$(printf '%s' "$f" | sed 's/-unsigned//')
      [ ! -e "$new" ] || fail "$new exists already"
    done
    : >"$signed_list.new"
    awk '{print $2}' "$list" | while IFS= read -r f; do
      new=$(printf '%s' "$f" | sed 's/-unsigned//')
      mv "$f" "$new"
      printf '%s  %s\n' "$(sha256 "$new")" "$new" >>"$signed_list.new"
    done
    LC_ALL=C sort -k2 "$signed_list.new" >"$signed_list"
    rm -f "$signed_list.new" "$list"
    sign_list "$signed_list" "${signed_list%.sha256}"
    echo "signed $signed_list ($sig_kind): $signed_list.$sig_ext"
    awk '{print "  " $2 "  " $1}' "$signed_list"
  done
  [ "$found" = 1 ] || fail "no ferris-idea-*-linux-*-unsigned.sha256 in $sign_dir"
  exit 0
fi

[ -n "$out" ] || usage
[ "$(uname -s)" = Linux ] || fail "the Linux packages are built on Linux (--sign runs anywhere)"
host_arch=$(uname -m)
case "$host_arch" in arm64) host_arch=aarch64 ;; esac
arch="${arch:-$host_arch}"
case "$arch" in
  x86_64) deb_arch=amd64 ;;
  aarch64) deb_arch=arm64 ;;
  *) fail "unknown architecture $arch (x86_64 or aarch64)" ;;
esac
[ "$arch" = "$host_arch" ] ||
  fail "--arch $arch on this $host_arch machine: the packages are built natively, not cross-compiled"
for tool in cargo rustc git tar gzip sha256sum md5sum dpkg-deb strip readelf curl find sort du; do
  command -v "$tool" >/dev/null 2>&1 || fail "$tool not found"
done

# Clean tree (behaviour 2), so the packages are exactly the commit they name.
[ -z "$(git status --porcelain --untracked-files=normal)" ] ||
  fail "uncommitted or untracked changes; commit or remove them first (git status)"
commit=$(git rev-parse HEAD)
version=$(cargo pkgid -p ferris-idea | sed 's/.*[#@]//')
case "$version" in [0-9]*.[0-9]*.[0-9]*) ;; *) fail "cannot read the version from Cargo: $version" ;; esac
tag_note="tag v$version is on this commit"
[ "$(git rev-parse -q --verify "refs/tags/v$version^{commit}" 2>/dev/null || true)" = "$commit" ] ||
  tag_note="warning: tag v$version is not on this commit"
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}"
case "$SOURCE_DATE_EPOCH" in '' | *[!0-9]*) fail "SOURCE_DATE_EPOCH must be a number of seconds" ;; esac
export SOURCE_DATE_EPOCH

if [ "$unsigned" = 1 ]; then
  [ -z "${FERRIS_IDEA_LINUX_MINISIGN_KEY:-}${FERRIS_IDEA_LINUX_GPG_KEY:-}" ] ||
    fail "--unsigned while a signing key is set; drop one of them"
  sfx=-unsigned
  signature="none (--unsigned: file names carry -unsigned)"
else
  choose_key
  sfx=""
  signature="$sig_kind (key from the environment)"
fi
printf 'commit %s, version %s (%s)\n' "$commit" "$version" "$tag_note"
printf 'architecture %s, signature: %s, SOURCE_DATE_EPOCH %s\n' "$arch" "$signature" "$SOURCE_DATE_EPOCH"

mkdir -p "$out"
out=$(cd "$out" && pwd -P)
target="${CARGO_TARGET_DIR:-target}"
mkdir -p "$target"
target=$(cd "$target" && pwd -P)
tools="${FERRIS_IDEA_RELEASE_TOOLS:-$target/release-tools}"
mkdir -p "$tools"
tools=$(cd "$tools" && pwd -P)

# Fetches one pinned file into the tools folder and checks its SHA-256 (behaviour 4).
fetch() { # fetch <url> <sha256> <name>
  if [ -f "$tools/$3" ] && [ "$(sha256 "$tools/$3")" = "$2" ]; then return 0; fi
  rm -f "$tools/$3" "$tools/$3.part"
  curl -fsSL --proto '=https' --tlsv1.2 --retry 3 -o "$tools/$3.part" "$1" || fail "download failed: $1"
  got=$(sha256 "$tools/$3.part")
  if [ "$got" != "$2" ]; then
    rm -f "$tools/$3.part"
    fail "$1: SHA-256 $got, expected $2; deleted"
  fi
  mv "$tools/$3.part" "$tools/$3"
}
step "pinned tools in $tools"
pinned_sums "$arch"
appimagetool="$tools/appimagetool-$APPIMAGETOOL_VERSION-$arch.AppImage"
fetch "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$arch.AppImage" \
  "$tool_sum" "$(basename "$appimagetool")"
chmod 755 "$appimagetool"
runtime="$tools/runtime-$RUNTIME_RELEASE-$arch"
fetch "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_RELEASE/runtime-$arch" \
  "$runtime_sum" "$(basename "$runtime")"
resvg="$tools/resvg-$RESVG_VERSION/bin/resvg"
if [ ! -x "$resvg" ]; then
  CARGO_TARGET_DIR="$tools/resvg-build" cargo install --quiet --locked --root "$tools/resvg-$RESVG_VERSION" \
    --version "$RESVG_VERSION" resvg || fail "cargo install resvg $RESVG_VERSION failed"
  rm -rf "$tools/resvg-build"
fi
echo "appimagetool $APPIMAGETOOL_VERSION, runtime $RUNTIME_RELEASE, resvg $("$resvg" --version)"

if [ "$check_only" = 1 ]; then
  echo "preconditions met; --check builds and signs nothing"
  exit 0
fi

# Release build (behaviour 5): no build path in the binary.
step "release build"
triple=$(rustc -vV | sed -n 's/^host: //p')
[ "$triple" = "$arch-unknown-linux-gnu" ] || fail "the Rust host is $triple, expected $arch-unknown-linux-gnu"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
cargo_home=$(cd "$cargo_home" && pwd -P)
case "$src$cargo_home" in *\"* | *\\*) fail "the source or Cargo folder has a quote or backslash in its name" ;; esac
cargo build --release --locked -p ferris-idea \
  --config "target.$triple.rustflags=[\"--remap-path-prefix=$src=/build/ferris-idea\", \"--remap-path-prefix=$cargo_home=/build/cargo\"]"
bin="$target/release/ferris-idea"
[ -x "$bin" ] || fail "$bin was not built"

# Only the script's own working folder is replaced; other files in --out stay.
work="$out/work-$arch"
rm -rf "$work"
mkdir -p "$work"
stamp() { find "$1" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +; }

# The contents of every package (behaviour 6), under one prefix.
step "contents"
p="$work/prefix"
mkdir -p "$p/bin" "$p/share/applications" "$p/share/icons/hicolor/scalable/apps" "$p/share/mime/packages" \
  "$p/share/metainfo" "$p/share/doc/ferris-idea"
strip --strip-all -o "$p/bin/ferris-idea" "$bin"
chmod 755 "$p/bin/ferris-idea"
cp packaging/icons/linux/ferris-idea.desktop "$p/share/applications/ferris-idea.desktop"
svg="$p/share/icons/hicolor/scalable/apps/ferris-idea.svg"
cp packaging/icons/linux/hicolor/scalable/apps/ferris-idea.svg "$svg"
for n in 16 24 32 48 64 128 256 512; do
  mkdir -p "$p/share/icons/hicolor/${n}x$n/apps"
  "$resvg" -w "$n" -h "$n" "$svg" "$p/share/icons/hicolor/${n}x$n/apps/ferris-idea.png" || fail "resvg could not render ${n}x$n"
done
cp packaging/linux/ferris-idea-mime.xml "$p/share/mime/packages/ferris-idea.xml"
release_date=$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y-%m-%d)
sed "s/@VERSION@/$version/; s/@DATE@/$release_date/" packaging/linux/dev.ferrisidea.FerrisIDEA.metainfo.xml \
  >"$p/share/metainfo/dev.ferrisidea.FerrisIDEA.metainfo.xml"
cp LICENSE.md "$p/share/doc/ferris-idea/copyright"
cp NOTICE THIRD-PARTY-NOTICES.md "$p/share/doc/ferris-idea/"
cp -R LICENSES "$p/share/doc/ferris-idea/LICENSES"
find "$p" -type d -exec chmod 755 {} +
find "$p" -type f ! -path "$p/bin/*" -exec chmod 644 {} +

# Checks of the contents (behaviours 8 and 13).
said=$("$p/bin/ferris-idea" --version) || fail "the binary does not start"
[ "${said%% (*}" = "ferris-idea $version" ] || fail "the binary says '$said', Cargo '$version'"
for lib in $(readelf -d "$p/bin/ferris-idea" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p'); do
  case "$lib" in
    libc.so.6 | libm.so.6 | libdl.so.2 | libpthread.so.0 | librt.so.1 | libutil.so.1 | ld-linux-*.so.*) ;;
    libgcc_s.so.1 | libstdc++.so.6 | libxcb.so.1 | libxkbcommon.so.0 | libxkbcommon-x11.so.0) ;;
    *) fail "the binary links to $lib, which the packages do not declare (SPEC-0354 behaviour 8)" ;;
  esac
done
glibc=$(readelf -V "$p/bin/ferris-idea" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' |
  sort -u -t. -k1,1n -k2,2n -k3,3n | tail -1)
[ -n "$glibc" ] || fail "cannot read the glibc version the binary needs"
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$p/share/applications/ferris-idea.desktop" || fail "desktop-file-validate rejects the desktop entry"
else
  echo "note: desktop-file-validate not found; the desktop entry is not validated"
fi
if command -v appstreamcli >/dev/null 2>&1; then
  appstreamcli validate --no-net "$p/share/metainfo/dev.ferrisidea.FerrisIDEA.metainfo.xml" ||
    fail "appstreamcli rejects the metainfo"
else
  echo "note: appstreamcli not found; the metainfo is not validated"
fi
printf '%s, needs glibc %s\n' "$said" "$glibc"

# Portable archive (behaviour 10).
step "portable archive"
name="ferris-idea-$version-linux-$arch"
mkdir -p "$work/tar/$name"
cp -R "$p/." "$work/tar/$name/"
cp packaging/linux/README.md "$work/tar/$name/README.md"
cp packaging/linux/install.sh "$work/tar/$name/install.sh"
chmod 644 "$work/tar/$name/README.md"
chmod 755 "$work/tar/$name/install.sh"
stamp "$work/tar"
tarball="$out/$name$sfx.tar.gz"
(cd "$work/tar" && tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
  --format=gnu -cf - "$name") | gzip -n -9 >"$tarball"
mkdir -p "$work/check-tar"
tar -xzf "$tarball" -C "$work/check-tar"
[ "$("$work/check-tar/$name/bin/ferris-idea" --version)" = "$said" ] || fail "the unpacked archive's binary does not start"

# Debian package (behaviour 12).
step "Debian package"
d="$work/deb"
mkdir -p "$d/usr" "$d/DEBIAN"
cp -R "$p/." "$d/usr/"
(cd "$d" && find usr -type f | LC_ALL=C sort | while IFS= read -r f; do md5sum "$f"; done) >"$d/DEBIAN/md5sums"
installed_size=$(du -sk "$d/usr" | cut -f1)
cat >"$d/DEBIAN/control" <<EOF
Package: ferris-idea
Version: $version
Architecture: $deb_arch
Maintainer: ${FERRIS_IDEA_DEB_MAINTAINER:-Ferris IDEA project ($HOMEPAGE)}
Installed-Size: $installed_size
Depends: libc6 (>= $glibc), libgcc-s1, libstdc++6, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libvulkan1, mesa-vulkan-drivers | vulkan-icd, fontconfig-config, fonts-dejavu-core | fonts-noto-core
Recommends: git
Suggests: gnome-keyring | keepassxc
Replaces: idecraft
Breaks: idecraft
Section: devel
Priority: optional
Homepage: $HOMEPAGE
Description: Experimental Rust-native IDE for code and databases
 Ferris IDEA edits, navigates, runs and debugs code in many languages, works
 with databases and offers its operations to AI agents through the Model
 Context Protocol. It sends no telemetry and does not look for updates by
 itself. Passwords are kept in the Secret Service (GNOME Keyring, KeePassXC);
 without one, Ferris IDEA refuses to store them.
EOF
chmod 644 "$d/DEBIAN/control" "$d/DEBIAN/md5sums"
stamp "$d"
deb="$out/ferris-idea_${version}_$deb_arch$sfx.deb"
dpkg-deb --root-owner-group -Zxz --build "$d" "$deb" >/dev/null || fail "dpkg-deb --build failed"
if [ "$(dpkg-deb -f "$deb" Package)" != ferris-idea ] || [ "$(dpkg-deb -f "$deb" Version)" != "$version" ] ||
  [ "$(dpkg-deb -f "$deb" Architecture)" != "$deb_arch" ]; then
  fail "the Debian package's control fields are wrong"
fi
dpkg-deb -f "$deb" Depends | grep -q "libc6 (>= $glibc)" || fail "the Debian package does not depend on libc6 >= $glibc"
! dpkg-deb --ctrl-tarfile "$deb" | tar -t | grep -q -E '(pre|post)(inst|rm)$|config$' ||
  fail "the Debian package has maintainer scripts"
mkdir -p "$work/check-deb"
dpkg-deb -x "$deb" "$work/check-deb"
[ "$("$work/check-deb/usr/bin/ferris-idea" --version)" = "$said" ] || fail "the Debian package's binary does not start"

# AppImage (behaviour 11): pinned appimagetool and runtime, no update information.
step "AppImage"
a="$work/AppDir"
mkdir -p "$a/usr"
cp -R "$p/." "$a/usr/"
cp packaging/linux/AppRun "$a/AppRun"
chmod 755 "$a/AppRun"
cp "$p/share/applications/ferris-idea.desktop" "$a/ferris-idea.desktop"
cp "$svg" "$a/ferris-idea.svg"
cp "$p/share/icons/hicolor/256x256/apps/ferris-idea.png" "$a/.DirIcon"
stamp "$a"
appimage="$out/ferris-idea-$version-$arch$sfx.AppImage"
rm -f "$appimage"
# A clean environment: no GITHUB_* or other variable from which appimagetool would derive update
# information, no network use (the runtime is given), the tool runs without FUSE.
if ! (cd "$work" && env -i PATH="$PATH" HOME="$HOME" TMPDIR="$work" ARCH="$arch" VERSION="$version" \
  SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" APPIMAGE_EXTRACT_AND_RUN=1 \
  "$appimagetool" --no-appstream --runtime-file "$runtime" "$a" "$appimage") >"$work/appimagetool.log" 2>&1; then
  tail -30 "$work/appimagetool.log" >&2
  fail "appimagetool failed"
fi
chmod 755 "$appimage"
[ ! -e "$appimage.zsync" ] || fail "appimagetool wrote a zsync file"
update_info=$("$appimage" --appimage-updateinformation 2>/dev/null || true)
[ -z "$update_info" ] || fail "the AppImage carries update information: $update_info"
[ "$(cd "$work" && TMPDIR="$work" "$appimage" --appimage-extract-and-run --version)" = "$said" ] ||
  fail "the AppImage's binary does not start"

# Checksums and signature (behaviour 14).
step "checksums"
sums="$out/ferris-idea-$version-linux-$arch$sfx.sha256"
(cd "$out" && for f in "$(basename "$tarball")" "$(basename "$appimage")" "$(basename "$deb")"; do
  printf '%s  %s\n' "$(sha256 "$f")" "$f"
done) | LC_ALL=C sort -k2 >"$sums"
if [ "$unsigned" = 0 ]; then
  sign_list "$sums" "ferris-idea $version linux $arch, commit $commit"
fi
[ "$(git rev-parse HEAD)" = "$commit" ] || fail "HEAD moved during the build"
rm -rf "$work"

step "done"
printf 'Ferris IDEA %s for Linux %s from commit %s (%s)\n' "$version" "$arch" "$commit" "$tag_note"
for f in "$tarball" "$appimage" "$deb" "$sums"; do
  printf '  %-48s %10s bytes  %s\n' "$(basename "$f")" "$(file_size "$f")" "$(sha256 "$f")"
done
if [ "$unsigned" = 0 ]; then
  printf '  %s (signature, %s)\n' "$(basename "$sums").$sig_ext" "$sig_kind"
else
  echo "unsigned: sign later with $0 --sign $out on a machine that holds the key"
fi
echo "nothing uploaded"
