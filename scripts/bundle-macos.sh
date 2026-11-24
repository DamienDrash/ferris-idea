#!/bin/sh
# Builds FerrisIDEA.app from the ferris-idea binary and checks it (SPEC-0282). Without
# FERRIS_IDEA_SIGN_IDENTITY the bundle is signed ad hoc and runs only on this Mac. With it, every
# nested Mach-O file and then the app are signed inside-out with that identity, the hardened runtime
# (packaging/macos/FerrisIDEA.entitlements) and a secure timestamp. There is no default identity.
# Notarizing and packaging the download: scripts/release-macos.sh. Nothing is uploaded.
#
#   bundle-macos.sh               release build (cargo build --release -p ferris-idea), then bundle
#   bundle-macos.sh --debug       bundle the debug binary instead (quick check in ci-local.sh)
#   bundle-macos.sh --no-build    bundle the binary that is already built
#   bundle-macos.sh --out DIR     put FerrisIDEA.app into DIR (default: <target>/bundle/macos)
#
#   FERRIS_IDEA_SIGN_IDENTITY=<name or SHA-1 of a signing identity in the keychain>
#
# Opens no window: the checks are plutil -lint, codesign --verify and `ferris-idea --version`.
# The identity is never printed: codesign's messages are shown with it replaced.
set -eu
# Variables under the former name (IDECRAFT_*) still count while today's is not set. rebrand:keep
for former in $(env | sed -n 's/^IDECRAFT_\([A-Za-z0-9_]*\)=.*/\1/p'); do # rebrand:keep
  eval "[ -n \"\${FERRIS_IDEA_$former+x}\" ] || { FERRIS_IDEA_$former=\$IDECRAFT_$former; export FERRIS_IDEA_$former; }" # rebrand:keep
done
export FERRIS_IDEA_NO_TAKEOVER=1 # the programs this script starts never move the user's settings folder (SPEC-0356)
cd "$(dirname "$0")/.."

profile=release
build=1
target="${CARGO_TARGET_DIR:-target}"
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) profile=debug ;;
    --no-build) build=0 ;;
    --out) out="$2"; shift ;;
    *) echo "usage: $0 [--debug] [--no-build] [--out DIR]" >&2; exit 2 ;;
  esac
  shift
done
[ "$(uname -s)" = Darwin ] || { echo "error: app bundles are built on macOS" >&2; exit 1; }
out="${out:-$target/bundle/macos}"

fail() { echo "error: $*" >&2; exit 1; }

if [ "$build" = 1 ]; then
  if [ "$profile" = release ]; then cargo build -q --release -p ferris-idea; else cargo build -q -p ferris-idea; fi
fi
binary="$target/$profile/ferris-idea"
[ -x "$binary" ] || fail "no binary at $binary (build it or leave out --no-build)"

# Version of the ferris-idea package in Cargo (behaviour 3).
version=$(cargo pkgid -p ferris-idea | sed 's/.*[#@]//')
case "$version" in [0-9]*.[0-9]*.[0-9]*) ;; *) fail "cannot read the version from Cargo: $version" ;; esac
# The oldest macOS the binary was linked for, read from its load commands.
min_os=$(otool -l "$binary" | awk '/LC_BUILD_VERSION/{b=1} b && $1=="minos"{print $2; exit}')
[ -n "$min_os" ] || fail "cannot read the minimum macOS version of $binary"

app="$out/FerrisIDEA.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/ferris-idea"
cp packaging/icons/macos/FerrisIDEA.icns "$app/Contents/Resources/FerrisIDEA.icns"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Exchange files of SPEC-0303 as document types: "<kind> <extension> <name>", one per line. A
# test in crates/formats checks that every extension of the registry is listed here.
kinds='connector idcconnector Ferris IDEA Database Connector
sources idcsources Ferris IDEA Data Sources
run idcrun Ferris IDEA Run Configuration
keymap idckeymap Ferris IDEA Keymap
theme idctheme Ferris IDEA Theme
code-style idcstyle Ferris IDEA Code Style
live-templates idctemplates Ferris IDEA Live Templates
settings idcsettings Ferris IDEA Settings
plugin idcplugin Ferris IDEA Plugin'
doc_types=""
exported=""
while read -r kind ext name; do
  [ -n "$kind" ] || continue
  conforms="public.utf8-plain-text"
  [ "$kind" = plugin ] && conforms="public.zip-archive"
  doc_types="$doc_types
    <dict>
      <key>CFBundleTypeName</key><string>$name</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Owner</string>
      <key>CFBundleTypeIconFile</key><string>FerrisIDEA</string>
      <key>LSItemContentTypes</key><array><string>dev.ferrisidea.$kind</string></array>
    </dict>"
  exported="$exported
    <dict>
      <key>UTTypeIdentifier</key><string>dev.ferrisidea.$kind</string>
      <key>UTTypeDescription</key><string>$name</string>
      <key>UTTypeConformsTo</key><array><string>$conforms</string><string>public.data</string></array>
      <key>UTTypeIconFile</key><string>FerrisIDEA</string>
      <key>UTTypeTagSpecification</key><dict><key>public.filename-extension</key><array><string>$ext</string></array></dict>
    </dict>"
done <<EOF
$kinds
EOF

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleName</key>
  <string>Ferris IDEA</string>
  <key>CFBundleDisplayName</key>
  <string>Ferris IDEA</string>
  <key>CFBundleIdentifier</key>
  <string>dev.ferrisidea.FerrisIDEA</string>
  <key>CFBundleExecutable</key>
  <string>ferris-idea</string>
  <key>CFBundleIconFile</key>
  <string>FerrisIDEA</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>LSMinimumSystemVersion</key>
  <string>$min_os</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSDesktopFolderUsageDescription</key>
  <string>Ferris IDEA opens, edits and searches the projects you keep on the Desktop.</string>
  <key>NSDocumentsFolderUsageDescription</key>
  <string>Ferris IDEA opens, edits and searches the projects you keep in Documents.</string>
  <key>NSDownloadsFolderUsageDescription</key>
  <string>Ferris IDEA opens, edits and searches the projects you keep in Downloads.</string>
  <key>NSRemovableVolumesUsageDescription</key>
  <string>Ferris IDEA opens projects on removable drives.</string>
  <key>NSNetworkVolumesUsageDescription</key>
  <string>Ferris IDEA opens projects on network drives.</string>
  <key>NSLocalNetworkUsageDescription</key>
  <string>Ferris IDEA connects to databases, containers, debuggers and remote hosts on your local network.</string>
  <key>NSAppleEventsUsageDescription</key>
  <string>A program you start in Ferris IDEA's terminal or run configurations wants to control another app.</string>
  <key>NSMicrophoneUsageDescription</key>
  <string>A program you start in Ferris IDEA's terminal or run configurations wants to use the microphone.</string>
  <key>NSCameraUsageDescription</key>
  <string>A program you start in Ferris IDEA's terminal or run configurations wants to use the camera.</string>
  <key>CFBundleDocumentTypes</key>
  <array>$doc_types
  </array>
  <key>UTExportedTypeDeclarations</key>
  <array>$exported
  </array>
</dict>
</plist>
PLIST

identity="${FERRIS_IDEA_SIGN_IDENTITY:-}"
entitlements=packaging/macos/FerrisIDEA.entitlements
# codesign's messages without the identity, a team ID in parentheses or a certificate hash.
redact() {
  awk -v id="$identity" '{ if (id != "") while ((i = index($0, id)) > 0) $0 = substr($0, 1, i - 1) "<FERRIS_IDEA_SIGN_IDENTITY>" substr($0, i + length(id)); print }' |
    sed -E 's/\([A-Z0-9]{10}\)/(<team>)/g; s/[0-9A-Fa-f]{40}/<hash>/g' >&2
}
sign() { # sign <path> [codesign options …]
  path="$1"; shift
  if ! codesign --force --options runtime --timestamp --sign "$identity" "$@" "$path" 2>"$app.sign.log"; then
    redact <"$app.sign.log"; rm -f "$app.sign.log"; fail "codesign failed for ${path#"$app"/}"
  fi
  rm -f "$app.sign.log"
}

if [ -z "$identity" ]; then
  # Ad-hoc signature (behaviour 4): accepted on this Mac, nothing to distribute.
  codesign --force --sign - --timestamp=none "$app" 2>/dev/null || fail "codesign failed"
  signed="ad-hoc signed (FERRIS_IDEA_SIGN_IDENTITY not set: runs on this Mac only, not for distribution)"
else
  # Signed with an identity (behaviour 4a): inside-out, deepest nested Mach-O file first, the
  # main executable last through the bundle itself. Today the binary is the only Mach-O file.
  [ -f "$entitlements" ] || fail "no entitlements file at $entitlements"
  plutil -lint -s "$entitlements" || fail "$entitlements is not valid"
  find "$app/Contents" -type f ! -path "$app/Contents/MacOS/ferris-idea" | while IFS= read -r f; do
    case "$(file -b "$f")" in *Mach-O*) printf '%s\t%s\n' "$(printf '%s' "$f" | tr -cd / | wc -c | tr -d ' ')" "$f" ;; esac
  done | sort -rn | cut -f2- | while IFS= read -r f; do sign "$f"; done
  sign "$app" --entitlements "$entitlements"
  details=$(codesign --display --verbose=2 "$app" 2>&1) || fail "cannot read the signature"
  printf '%s\n' "$details" | grep -q '^CodeDirectory.*flags=.*(runtime)' || fail "the app is signed without the hardened runtime"
  printf '%s\n' "$details" | grep -q '^Timestamp=' || fail "the signature has no secure timestamp"
  codesign --verify --deep --strict "$app" || fail "the signature does not verify (--deep --strict)"
  for key in automation.apple-events device.audio-input device.camera; do
    codesign --display --entitlements - --xml "$app" 2>/dev/null | grep -q "com.apple.security.$key" ||
      fail "entitlement com.apple.security.$key is missing in the signature"
  done
  kind=$(printf '%s\n' "$details" | sed -n 's/^Authority=\([^:]*\):.*/\1/p' | head -n 1)
  signed="signed with the identity from FERRIS_IDEA_SIGN_IDENTITY (${kind:-unknown kind}), hardened runtime, timestamp"
fi

# Checks (behaviour 5). With an identity, `--version` is also the hardened-runtime launch check.
plutil -lint -s "$app/Contents/Info.plist" || fail "Info.plist is not valid"
codesign --verify --strict "$app" || fail "the signature does not verify"
cmp -s "$app/Contents/Resources/FerrisIDEA.icns" packaging/icons/macos/FerrisIDEA.icns || fail "icon differs from packaging/icons/macos/FerrisIDEA.icns"
types=$(plutil -extract UTExportedTypeDeclarations raw "$app/Contents/Info.plist")
[ "$types" = "$(printf '%s\n' "$kinds" | grep -c .)" ] || fail "Info.plist declares $types exported types, the script lists $(printf '%s\n' "$kinds" | grep -c .)"
plutil -extract CFBundleDocumentTypes.2.LSItemContentTypes.0 raw "$app/Contents/Info.plist" | grep -qx dev.ferrisidea.run || fail "the run configuration document type is missing"
plist_version=$(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist")
said=$("$app/Contents/MacOS/ferris-idea" --version)
[ "${said%% (*}" = "ferris-idea $plist_version" ] || fail "the bundle's binary says '$said', the plist '$plist_version'"
printf 'built %s: version %s, macOS %s or later, %s; checks passed\n' "$app" "$version" "$min_os" "$signed"
