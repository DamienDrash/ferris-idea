#!/bin/sh
# Builds the signed, notarized and stapled macOS download of IDECraft (SPEC-0282 v3, behaviours
# 9–15): release build, IDECraft.app signed with a Developer ID Application identity and the
# hardened runtime (scripts/bundle-macos.sh), notarized and stapled; then a DMG with the app,
# signed, notarized and stapled; then the checks Gatekeeper makes, and the DMG's SHA-256.
#
# It uploads nothing to GitHub or anywhere else except the notary service: publishing the DMG is
# a separate step that needs the project owner's approval. One-time setup and the release steps:
# docs/RELEASE_MACOS.md.
#
#   release-macos.sh             build, sign, notarize, staple, check; prints commit, version, SHA-256
#   release-macos.sh --check     only the preconditions: clean tree, identity, notary credentials
#   release-macos.sh --out DIR   where the DMG goes (default: <target>/release-macos)
#
# Read from the environment at run time, never stored or printed:
#   IDECRAFT_SIGN_IDENTITY    name or SHA-1 of a "Developer ID Application" identity in the keychain
#   IDECRAFT_NOTARY_PROFILE   a profile stored with `xcrun notarytool store-credentials`, or
#   IDECRAFT_NOTARY_KEY, IDECRAFT_NOTARY_KEY_ID, IDECRAFT_NOTARY_ISSUER
#                             an App Store Connect API key: path of the .p8 file (readable by its
#                             owner only), key ID and issuer ID
set -eu
cd "$(dirname "$0")/.."

check_only=0
target="${CARGO_TARGET_DIR:-target}"
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --check) check_only=1 ;;
    --out) out="$2"; shift ;;
    *) echo "usage: $0 [--check] [--out DIR]" >&2; exit 2 ;;
  esac
  shift
done
out="${out:-$target/release-macos}"

fail() { echo "error: $*" >&2; exit 1; }
step() { printf '\n== %s\n' "$*"; }

[ "$(uname -s)" = Darwin ] || fail "the macOS download is built on macOS"
for tool in codesign hdiutil ditto shasum spctl security; do
  command -v "$tool" >/dev/null || fail "$tool not found"
done
xcrun --find notarytool >/dev/null 2>&1 || fail "xcrun notarytool not found (install Xcode or its command line tools)"
xcrun --find stapler >/dev/null 2>&1 || fail "xcrun stapler not found"

# 1. A clean tree, so the download is exactly the commit it names (behaviour 9).
[ -z "$(git status --porcelain --untracked-files=normal)" ] ||
  fail "uncommitted or untracked changes; commit or remove them first (git status)"
commit=$(git rev-parse HEAD)
version=$(cargo pkgid -p idecraft | sed 's/.*[#@]//')
case "$version" in [0-9]*.[0-9]*.[0-9]*) ;; *) fail "cannot read the version from Cargo: $version" ;; esac
tag_note="tag v$version is on this commit"
[ "$(git rev-parse -q --verify "refs/tags/v$version^{commit}" 2>/dev/null || true)" = "$commit" ] ||
  tag_note="warning: tag v$version is not on this commit"

# 2. A Developer ID Application identity (behaviour 10). Matched by SHA-1 or by its full quoted
#    name in the keychain's list of valid signing identities; the kind is the part before ':'.
identity="${IDECRAFT_SIGN_IDENTITY:-}"
[ -n "$identity" ] || fail "IDECRAFT_SIGN_IDENTITY is not set (name or SHA-1 of a Developer ID Application identity)"
kind=$(security find-identity -v -p codesigning | awk -v id="$identity" '
  $1 ~ /^[0-9]+\)$/ {
    name = $0; sub(/^[^"]*"/, "", name); sub(/"[^"]*$/, "", name)
    if ($2 == id || toupper($2) == toupper(id) || name == id) { split(name, part, ":"); print part[1] }
  }' | sort -u)
case "$kind" in
  "Developer ID Application") ;;
  "") fail "no valid signing identity in the keychain matches IDECRAFT_SIGN_IDENTITY" ;;
  *) fail "IDECRAFT_SIGN_IDENTITY is a '$(printf '%s' "$kind" | head -n 1)' identity; a download needs 'Developer ID Application'" ;;
esac

# 3. Notary credentials: a keychain profile or the API-key trio, not both (behaviour 11).
profile="${IDECRAFT_NOTARY_PROFILE:-}"
key="${IDECRAFT_NOTARY_KEY:-}"
key_id="${IDECRAFT_NOTARY_KEY_ID:-}"
issuer="${IDECRAFT_NOTARY_ISSUER:-}"
if [ -n "$profile" ]; then
  [ -z "$key$key_id$issuer" ] || fail "set either IDECRAFT_NOTARY_PROFILE or the IDECRAFT_NOTARY_KEY trio, not both"
  notary_kind="keychain profile"
elif [ -n "$key" ] || [ -n "$key_id" ] || [ -n "$issuer" ]; then
  [ -n "$key" ] && [ -n "$key_id" ] && [ -n "$issuer" ] ||
    fail "IDECRAFT_NOTARY_KEY, IDECRAFT_NOTARY_KEY_ID and IDECRAFT_NOTARY_ISSUER are needed together"
  [ -f "$key" ] || fail "IDECRAFT_NOTARY_KEY does not name a file"
  mode=$(stat -f %Lp "$key")
  case "$mode" in ?00) ;; *) fail "the API key file is readable by others (mode $mode); chmod 600 it" ;; esac
  notary_kind="App Store Connect API key"
else
  fail "no notary credentials: set IDECRAFT_NOTARY_PROFILE, or IDECRAFT_NOTARY_KEY, IDECRAFT_NOTARY_KEY_ID and IDECRAFT_NOTARY_ISSUER"
fi
notarytool() { # notarytool <subcommand> [args …], with the credentials added
  sub="$1"; shift
  if [ -n "$profile" ]; then
    xcrun notarytool "$sub" "$@" --keychain-profile "$profile"
  else
    xcrun notarytool "$sub" "$@" --key "$key" --key-id "$key_id" --issuer "$issuer"
  fi
}

printf 'commit %s, version %s (%s)\n' "$commit" "$version" "$tag_note"
printf 'identity: %s (from IDECRAFT_SIGN_IDENTITY), notary: %s\n' "$kind" "$notary_kind"
if [ "$check_only" = 1 ]; then
  echo "preconditions met; --check builds, signs and sends nothing"
  exit 0
fi

# Submits a file and waits for the verdict; prints the submission ID and fails unless accepted.
notarize() {
  step "notarize $(basename "$1") (usually a few minutes)"
  result="$work/notary-$(basename "$1").json"
  notarytool submit "$1" --wait --output-format json >"$result" || true
  id=$(plutil -extract id raw "$result" 2>/dev/null || echo "?")
  status=$(plutil -extract status raw "$result" 2>/dev/null || echo "no answer")
  printf 'submission %s: %s\n' "$id" "$status"
  [ "$status" = Accepted ] ||
    fail "notarization of $(basename "$1") was not accepted; details: xcrun notarytool log $id with the same credentials"
}

# Only the script's own working folder is replaced; other files in --out stay.
work="$out/work"
rm -rf "$work"
mkdir -p "$work"

# 4. Release build, bundle, sign (behaviour 10): scripts/bundle-macos.sh with the identity.
step "build and sign IDECraft.app"
IDECRAFT_SIGN_IDENTITY="$identity" scripts/bundle-macos.sh --out "$work"
app="$work/IDECraft.app"
case "$(lipo -archs "$app/Contents/MacOS/idecraft")" in
  arm64) arch=arm64 ;;
  x86_64) arch=x86_64 ;;
  *x86_64*arm64* | *arm64*x86_64*) arch=universal ;;
  *) fail "unknown architecture of the binary" ;;
esac

# 5. Notarize and staple the app, so a copy outside the DMG opens offline too (behaviour 12).
ditto -c -k --keepParent "$app" "$work/IDECraft.zip"
notarize "$work/IDECraft.zip"
xcrun stapler staple -q "$app" || fail "stapling the app failed"

# 6. The DMG: the stapled app and a link to /Applications, compressed, signed (behaviour 13).
step "DMG"
dmg="$out/IDECraft-$version-macos-$arch.dmg"
stage="$work/dmg"
mkdir -p "$stage"
ditto "$app" "$stage/IDECraft.app"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "IDECraft $version" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg" ||
  fail "hdiutil create failed"
codesign --force --timestamp --sign "$identity" "$dmg" 2>"$work/dmg-sign.log" || {
  sed -E 's/\([A-Z0-9]{10}\)/(<team>)/g; s/[0-9A-Fa-f]{40}/<hash>/g' "$work/dmg-sign.log" >&2
  fail "signing the DMG failed"
}
notarize "$dmg"
xcrun stapler staple -q "$dmg" || fail "stapling the DMG failed"

# 7. Checks (behaviour 14): what Gatekeeper checks on a downloaded file, on the app and on the DMG,
#    and the app inside the DMG as the user gets it.
step "checks"
codesign --verify --deep --strict "$app" || fail "app: codesign --verify --deep --strict"
spctl --assess --type execute "$app" || fail "app: Gatekeeper rejects it (spctl --type execute)"
xcrun stapler validate -q "$app" || fail "app: no valid stapled ticket"
codesign --verify --strict "$dmg" || fail "DMG: codesign --verify --strict"
spctl --assess --type open --context context:primary-signature "$dmg" || fail "DMG: Gatekeeper rejects it (spctl --type open)"
xcrun stapler validate -q "$dmg" || fail "DMG: no valid stapled ticket"
mnt="$work/mnt"
mkdir -p "$mnt"
hdiutil attach -quiet -nobrowse -readonly -noautoopen -mountpoint "$mnt" "$dmg" || fail "cannot mount the DMG"
inside=0
codesign --verify --deep --strict "$mnt/IDECraft.app" &&
  spctl --assess --type execute "$mnt/IDECraft.app" &&
  xcrun stapler validate -q "$mnt/IDECraft.app" &&
  said=$("$mnt/IDECraft.app/Contents/MacOS/idecraft" --version) &&
  [ "${said%% (*}" = "idecraft $version" ] && inside=1
hdiutil detach -quiet "$mnt" || hdiutil detach -quiet -force "$mnt" || true
[ "$inside" = 1 ] || fail "the app inside the DMG does not pass the checks or does not start"
[ "$(git rev-parse HEAD)" = "$commit" ] || fail "HEAD moved during the build"

sum=$(shasum -a 256 "$dmg" | awk '{print $1}')
printf '%s  %s\n' "$sum" "$(basename "$dmg")" >"$dmg.sha256"
rm -rf "$work"
step "done"
printf 'IDECraft %s from commit %s (%s)\n' "$version" "$commit" "$tag_note"
printf 'artifact: %s\nSHA-256:  %s\n' "$dmg" "$sum"
echo "signed (Developer ID Application, hardened runtime), notarized and stapled; nothing uploaded"
