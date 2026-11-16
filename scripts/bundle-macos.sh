#!/bin/sh
# Builds a local IDECraft.app from the idecraft binary and checks it (SPEC-0282). Ad-hoc signed
# only: no Developer ID, no notarization, no upload. Distributing the bundle needs GO-4.
#
#   bundle-macos.sh               release build (cargo build --release -p idecraft), then bundle
#   bundle-macos.sh --debug       bundle the debug binary instead (quick check in ci-local.sh)
#   bundle-macos.sh --no-build    bundle the binary that is already built
#   bundle-macos.sh --out DIR     put IDECraft.app into DIR (default: <target>/bundle/macos)
#
# Opens no window: the checks are plutil -lint, codesign --verify and `idecraft --version`.
set -eu
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
  if [ "$profile" = release ]; then cargo build -q --release -p idecraft; else cargo build -q -p idecraft; fi
fi
binary="$target/$profile/idecraft"
[ -x "$binary" ] || fail "no binary at $binary (build it or leave out --no-build)"

# Version of the idecraft package in Cargo (behaviour 3).
version=$(cargo pkgid -p idecraft | sed 's/.*[#@]//')
case "$version" in [0-9]*.[0-9]*.[0-9]*) ;; *) fail "cannot read the version from Cargo: $version" ;; esac
# The oldest macOS the binary was linked for, read from its load commands.
min_os=$(otool -l "$binary" | awk '/LC_BUILD_VERSION/{b=1} b && $1=="minos"{print $2; exit}')
[ -n "$min_os" ] || fail "cannot read the minimum macOS version of $binary"

app="$out/IDECraft.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/idecraft"
cp packaging/icons/macos/IDECraft.icns "$app/Contents/Resources/IDECraft.icns"
printf 'APPL????' > "$app/Contents/PkgInfo"

# Exchange files of SPEC-0303 as document types: "<kind> <extension> <name>", one per line. A
# test in crates/formats checks that every extension of the registry is listed here.
kinds='connector idcconnector IDECraft Database Connector
sources idcsources IDECraft Data Sources
run idcrun IDECraft Run Configuration
keymap idckeymap IDECraft Keymap
theme idctheme IDECraft Theme
code-style idcstyle IDECraft Code Style
live-templates idctemplates IDECraft Live Templates
settings idcsettings IDECraft Settings
plugin idcplugin IDECraft Plugin'
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
      <key>CFBundleTypeIconFile</key><string>IDECraft</string>
      <key>LSItemContentTypes</key><array><string>dev.idecraft.$kind</string></array>
    </dict>"
  exported="$exported
    <dict>
      <key>UTTypeIdentifier</key><string>dev.idecraft.$kind</string>
      <key>UTTypeDescription</key><string>$name</string>
      <key>UTTypeConformsTo</key><array><string>$conforms</string><string>public.data</string></array>
      <key>UTTypeIconFile</key><string>IDECraft</string>
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
  <string>IDECraft</string>
  <key>CFBundleDisplayName</key>
  <string>IDECraft</string>
  <key>CFBundleIdentifier</key>
  <string>dev.idecraft.IDECraft</string>
  <key>CFBundleExecutable</key>
  <string>idecraft</string>
  <key>CFBundleIconFile</key>
  <string>IDECraft</string>
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
  <string>IDECraft opens, edits and searches the projects you keep on the Desktop.</string>
  <key>NSDocumentsFolderUsageDescription</key>
  <string>IDECraft opens, edits and searches the projects you keep in Documents.</string>
  <key>NSDownloadsFolderUsageDescription</key>
  <string>IDECraft opens, edits and searches the projects you keep in Downloads.</string>
  <key>NSRemovableVolumesUsageDescription</key>
  <string>IDECraft opens projects on removable drives.</string>
  <key>NSNetworkVolumesUsageDescription</key>
  <string>IDECraft opens projects on network drives.</string>
  <key>NSLocalNetworkUsageDescription</key>
  <string>IDECraft connects to databases, containers, debuggers and remote hosts on your local network.</string>
  <key>NSAppleEventsUsageDescription</key>
  <string>A program you start in IDECraft's terminal or run configurations wants to control another app.</string>
  <key>NSMicrophoneUsageDescription</key>
  <string>A program you start in IDECraft's terminal or run configurations wants to use the microphone.</string>
  <key>NSCameraUsageDescription</key>
  <string>A program you start in IDECraft's terminal or run configurations wants to use the camera.</string>
  <key>CFBundleDocumentTypes</key>
  <array>$doc_types
  </array>
  <key>UTExportedTypeDeclarations</key>
  <array>$exported
  </array>
</dict>
</plist>
PLIST

# Ad-hoc signature (behaviour 4): accepted on this Mac, nothing to distribute.
codesign --force --sign - --timestamp=none "$app" 2>/dev/null || fail "codesign failed"

# Checks (behaviour 5).
plutil -lint -s "$app/Contents/Info.plist" || fail "Info.plist is not valid"
codesign --verify --strict "$app" || fail "the signature does not verify"
cmp -s "$app/Contents/Resources/IDECraft.icns" packaging/icons/macos/IDECraft.icns || fail "icon differs from packaging/icons/macos/IDECraft.icns"
types=$(plutil -extract UTExportedTypeDeclarations raw "$app/Contents/Info.plist")
[ "$types" = "$(printf '%s\n' "$kinds" | grep -c .)" ] || fail "Info.plist declares $types exported types, the script lists $(printf '%s\n' "$kinds" | grep -c .)"
plutil -extract CFBundleDocumentTypes.2.LSItemContentTypes.0 raw "$app/Contents/Info.plist" | grep -qx dev.idecraft.run || fail "the run configuration document type is missing"
plist_version=$(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist")
said=$("$app/Contents/MacOS/idecraft" --version)
[ "${said%% (*}" = "idecraft $plist_version" ] || fail "the bundle's binary says '$said', the plist '$plist_version'"
printf 'built %s: version %s, macOS %s or later, ad-hoc signed, checks passed\n' "$app" "$version" "$min_os"
