#!/usr/bin/env bash
# Builds crates/languages/java/debug-private/idecraft-java-debug-private.jar (SPEC-0322 behaviour 4)
# from our sources. The EPL-licensed jars below are only compiled against and are not packaged.
# Sources are pinned by version and SHA-256; timestamps are fixed, so the same JDK gives the same
# bytes. Built with Temurin 17.0.20.1.
#
# Usage: scripts/build-java-debug-private.sh [--check]
#   --check   build into a temporary folder and fail if the result differs from the committed jar.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bundle="$root/crates/languages/java/debug-private"
target="$bundle/idecraft-java-debug-private.jar"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

maven=https://repo1.maven.org/maven2
fetch() { # url sha256 name
    curl -fsSL "$1" -o "$work/$3"
    echo "$2  $work/$3" | shasum -a 256 -c - >/dev/null
}

fetch "$maven/com/microsoft/java/com.microsoft.java.debug.plugin/0.53.1/com.microsoft.java.debug.plugin-0.53.1.jar" \
    4f4778d452a6a0665536f43ce4e32403a24be6593336b80dc85a322912859e24 plugin.jar
fetch "$maven/org/eclipse/jdt/org.eclipse.jdt.debug/3.26.100/org.eclipse.jdt.debug-3.26.100.jar" \
    585fe0f754e8b364258abfb5733e65bf1da895b9c5d628d577e7cf7e6a421915 jdt-debug.jar
fetch "$maven/org/eclipse/platform/org.eclipse.equinox.common/3.21.0/org.eclipse.equinox.common-3.21.0.jar" \
    2545ed27d408f19882695703097e5998bf25652bd0dac76a8e6d1391ec228ab7 equinox-common.jar

fetch "$maven/org/eclipse/platform/org.eclipse.osgi/3.24.300/org.eclipse.osgi-3.24.300.jar" \
    4a770aa4a78df653af871073c8b4ddd56884895f5fc71d10c40b4289ac7c8b9f osgi.jar

(cd "$work" && unzip -q -o plugin.jar 'lib/com.microsoft.java.debug.core-*.jar' && unzip -q -o jdt-debug.jar jdimodel.jar)
core="$(ls "$work"/lib/com.microsoft.java.debug.core-*.jar)"

mkdir -p "$work/classes/impl" "$work/stubs"
javac -nowarn --release 17 -d "$work/stubs" \
    -cp "$work/equinox-common.jar" \
    "$bundle/stubs/org/eclipse/jdt/ls/core/internal/IDelegateCommandHandler.java"
# The real handler, kept under impl/ and defined in a loader on top of java-debug's.
find "$bundle/src" -name '*.java' | LC_ALL=C sort > "$work/files"
javac -nowarn --release 17 -d "$work/classes/impl" \
    -cp "$work/plugin.jar:$core:$work/jdimodel.jar:$work/equinox-common.jar:$work/stubs" \
    @"$work/files"
# The entry jdtls loads.
find "$bundle/boot" -name '*.java' | LC_ALL=C sort > "$work/files"
javac -nowarn --release 17 -d "$work/classes" \
    -cp "$work/osgi.jar:$work/equinox-common.jar:$work/stubs" \
    @"$work/files"
cp "$bundle/plugin.xml" "$work/classes/"

out="$work/out.jar"
(cd "$work/classes" && find . -type f | LC_ALL=C sort | sed 's#^\./##' > "$work/entries"
 jar --create --file "$out" --date=2026-11-09T00:00:00Z --manifest "$bundle/META-INF/MANIFEST.MF" @"$work/entries")

if [ "${1:-}" = "--check" ]; then
    cmp "$out" "$target"
    echo "reproduced: $(shasum -a 256 "$target" | cut -d' ' -f1)"
else
    cp "$out" "$target"
    echo "built: $(shasum -a 256 "$target" | cut -d' ' -f1)"
fi
