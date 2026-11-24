#!/bin/sh
# Window self-test (`ferris-idea ui --selftest`) on Linux, in a local Lima VM on the Mac (QG-1
# criterion 3). The build server has no X server and no Vulkan driver and must not get system
# packages, so the Linux window run happens here instead: Ubuntu 24.04 aarch64 under Apple's
# virtualization (vz), an X server in memory (Xvfb) and Mesa's software Vulkan driver (lavapipe).
#
#   linux-ui-selftest-lima.sh              create or start the VM, provision it, copy the tree,
#                                          build `ferris-idea` (debug), run the self-test, stop the VM
#   linux-ui-selftest-lima.sh --keep       the same, but leave the VM running afterwards
#   linux-ui-selftest-lima.sh --build-only stop after the build (VM keeps running)
#
# The VM (FERRIS_IDEA_LIMA_VM, default idecraft-linux-ui) is the only one this script creates, starts
# or stops; it never deletes a VM and never touches another one. It is made in Lima's plain mode:
# no folder of the Mac is mounted, no port of the guest is forwarded, no containerd. Lima's own SSH
# listens on 127.0.0.1 only. The source goes in as a copy (rsync over that SSH: the tree without
# .git and without what .gitignore names), so nothing in the VM can change the worktree. Packages
# come from the Ubuntu archive only (rustup as well; the toolchain then follows rust-toolchain.toml).
# Nothing secret is read, written or printed.
#
# What it prints: the build log tail on failure, the lines of groups and tools the self-test skipped,
# the slowest groups and the result line `{"checks":N,"failed":[…],"skipped":[…]}`. The logs
# (build.log, selftest.log, result.json) are in $FERRIS_IDEA_LIMA_OUT (default
# target/linux-ui-selftest/) on the Mac. Exit code 0 only when the result
# line was printed and `failed` is empty.
#
# Environment passed into the VM: FERRIS_IDEA_SELFTEST_ONLY, FERRIS_IDEA_SELFTEST_SKIP,
# FERRIS_IDEA_SELFTEST_DRIVER, FERRIS_IDEA_SELFTEST_TRACE. SELFTEST_TIMEOUT (seconds, default 10800)
# bounds the self-test: on the software renderer a full run takes far longer than on the Mac. The
# first debug build in the VM takes about 25 minutes.
set -eu
# Variables under the former name (IDECRAFT_*) still count while today's is not set. rebrand:keep
for former in $(env | sed -n 's/^IDECRAFT_\([A-Za-z0-9_]*\)=.*/\1/p'); do # rebrand:keep
  eval "[ -n \"\${FERRIS_IDEA_$former+x}\" ] || { FERRIS_IDEA_$former=\$IDECRAFT_$former; export FERRIS_IDEA_$former; }" # rebrand:keep
done
cd "$(dirname "$0")/.."

VM="${FERRIS_IDEA_LIMA_VM:-idecraft-linux-ui}"
LIMACTL="${LIMACTL:-$(command -v limactl || echo "$HOME/.local/bin/limactl")}"
OUT="${FERRIS_IDEA_LIMA_OUT:-target/linux-ui-selftest}"
TIMEOUT="${SELFTEST_TIMEOUT:-10800}"
# Bump when the package list changes: the VM is provisioned again.
PROVISION=2
# Bump when a pinned tool changes: the tools are provisioned again.
TOOLS=3

keep=0; build_only=0
for a in "$@"; do
  case "$a" in
    --keep) keep=1 ;;
    --build-only) build_only=1; keep=1 ;;
    *) echo "usage: linux-ui-selftest-lima.sh [--keep | --build-only]" >&2; exit 2 ;;
  esac
done

case "$VM" in
  idecraft-db | idecraft-docker | idecraft-k3s | idecraft-mssql)
    echo "error: $VM belongs to the reference environment; choose another FERRIS_IDEA_LIMA_VM" >&2; exit 2 ;;
esac

mkdir -p "$OUT"
status() { "$LIMACTL" list --format '{{.Status}}' "$VM" 2>/dev/null || true; }
in_vm() { "$LIMACTL" shell --workdir / "$VM" -- "$@"; }

if [ -z "$(status)" ]; then
  echo "== creating VM $VM (Ubuntu 24.04 aarch64, vz, 6 CPUs, 12 GiB, 60 GiB, plain mode)"
  "$LIMACTL" create --tty=false --name "$VM" --plain --vm-type vz --arch aarch64 \
    --cpus 6 --memory 12 --disk 60 template:ubuntu-24.04
fi
if [ "$(status)" != Running ]; then
  echo "== starting VM $VM"
  "$LIMACTL" start --tty=false "$VM"
fi
if [ "$keep" = 0 ]; then
  trap 'echo "== stopping VM $VM"; "$LIMACTL" stop "$VM" >/dev/null 2>&1 || true' EXIT
fi

echo "== provisioning (packages from the Ubuntu archive)"
in_vm bash -s "$PROVISION" <<'EOF'
set -eu
marker="$HOME/.idecraft-provisioned-$1"
[ -e "$marker" ] && { echo "already provisioned ($1)"; exit 0; }
export DEBIAN_FRONTEND=noninteractive
sudo apt-get update -q
# Build: compiler, lld (.cargo/config.toml links aarch64 Linux with it), pkg-config, the system libraries of gpui's X11/Wayland backends and of the -sys
# crates (openssl, fontconfig/freetype, xkbcommon, xcb, Vulkan loader).
# Window run: Xvfb, Mesa's software Vulkan (lavapipe), a font.
# Tools the self-test groups use: git, sqlite3, lsof, cc, lldb-dap, go and gopls, Maven and a JDK,
# node and npm, python3 with debugpy and pylsp, clangd.
sudo apt-get install -y -q --no-install-recommends \
  build-essential pkg-config cmake clang lld rsync jq curl ca-certificates rustup \
  libssl-dev libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-dev libx11-xcb-dev libxcb1-dev libvulkan-dev libvulkan1 \
  xvfb xauth x11-utils mesa-vulkan-drivers vulkan-tools fonts-dejavu-core \
  git sqlite3 lsof lldb-18 golang-go gopls maven openjdk-21-jdk-headless nodejs npm \
  python3 python3-debugpy python3-pylsp clangd
rustup set profile minimal
touch "$marker"
EOF

# Tools the Ubuntu archive does not have in the version the self-test needs, from their projects'
# own distribution places, each pinned: a download is checked against its SHA-256 (published by the
# project, or the one the Mac's copy has) before it is unpacked; the LLVM archive key is checked by
# its fingerprint. Everything lands in the VM user's ~/.local (the adapters' default places) except
# the packages of the archives. What it fills and why:
#   clangd 21 (apt.llvm.org)      Call Hierarchy, Extract Method and the Problems list of the C/C++
#                                 groups answer like the Mac's clangd 21; Ubuntu's 18 does not
#   perf (linux-tools)            profiler-attach samples with perf on Linux
#   JDK 25, jdtls 1.64.0, java-debug 0.53.1   the java-* groups (jdtls 1.64 needs Java 25)
#   node 24.18.0, js-debug 1.95.3 node-debug, package-script-debug (node 22.18+); browser-debug also
#                                 needs Chrome, which Google does not build for Linux on ARM
#   pyright 1.1.414 (npm)       the Python groups' diagnostics (pylsp reports no unresolved import)
#   Gradle 8.10.2, the Kotlin Gradle plugin 2.0.21 in Gradle's cache, kotlin-debug-adapter 0.4.4
#                                 kotlin-debug
#   ruby, ruby-lsp 0.26.11 (rubygems.org)  ruby-server
echo "== provisioning tools (official upstream sources, pinned)"
in_vm bash -s "$TOOLS" <<'EOF'
set -eu
marker="$HOME/.idecraft-tools-$1"
[ -e "$marker" ] && { echo "tools already provisioned ($1)"; exit 0; }
export DEBIAN_FRONTEND=noninteractive
opt="$HOME/.local/opt"; bin="$HOME/.local/bin"
mkdir -p "$opt" "$bin"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
fetch() { # url sha256 file
  curl -fsSL --proto '=https' --tlsv1.2 -o "$tmp/$3" "$1"
  echo "$2  $tmp/$3" | sha256sum -c --quiet -
}

# LLVM's own archive for clangd 21; the key must have LLVM's published fingerprint.
curl -fsSL --proto '=https' https://apt.llvm.org/llvm-snapshot.gpg.key >"$tmp/llvm.key"
gpg --show-keys --with-colons "$tmp/llvm.key" | grep -q '^fpr:::::::::6084F3CF814B57C1CF12EFD515CF4D18AF4F7421:' \
  || { echo "error: the LLVM archive key has an unexpected fingerprint" >&2; exit 1; }
gpg --dearmor <"$tmp/llvm.key" | sudo tee /usr/share/keyrings/llvm-archive.gpg >/dev/null
echo 'deb [signed-by=/usr/share/keyrings/llvm-archive.gpg] https://apt.llvm.org/noble/ llvm-toolchain-noble-21 main' \
  | sudo tee /etc/apt/sources.list.d/llvm-21.list >/dev/null
sudo apt-get update -q
sudo apt-get install -y -q --no-install-recommends clangd-21 openjdk-25-jdk-headless \
  "linux-tools-$(uname -r)" linux-tools-common ruby ruby-dev ruby-bundler unzip
ln -sf /usr/bin/clangd-21 "$bin/clangd"
# JDK 25 is for jdtls only; java stays 21 (Gradle 8.10 does not run on 25).
for t in java javac; do
  sudo update-alternatives --set "$t" "/usr/lib/jvm/java-21-openjdk-arm64/bin/$t"
done

# jdtls (Eclipse), java-debug (Maven Central, the jar the Mac uses), JDK 25 for jdtls.
fetch https://download.eclipse.org/jdtls/snapshots/jdt-language-server-1.64.0-202610301801.tar.gz \
  3bff77299e7449ddec5466e1c600a5f221312b057240b06162b7d9326bb9ca5e jdtls.tgz
rm -rf "$opt/jdtls"; mkdir -p "$opt/jdtls"; tar -xzf "$tmp/jdtls.tgz" -C "$opt/jdtls"
printf '#!/bin/sh\n# jdtls with the JDK 25 it needs (the default java is 21).\nJAVA_HOME=/usr/lib/jvm/java-25-openjdk-arm64 exec "%s/jdtls/bin/jdtls" "$@"\n' "$opt" >"$bin/jdtls"
chmod +x "$bin/jdtls"
mkdir -p "$opt/java-debug"
fetch https://repo1.maven.org/maven2/com/microsoft/java/com.microsoft.java.debug.plugin/0.53.1/com.microsoft.java.debug.plugin-0.53.1.jar \
  4f4778d452a6a0665536f43ce4e32403a24be6593336b80dc85a322912859e24 java-debug.jar
cp "$tmp/java-debug.jar" "$opt/java-debug/com.microsoft.java.debug.plugin-0.53.1.jar"

# Node.js (nodejs.org), js-debug (Microsoft's release), pyright (npm registry).
fetch https://nodejs.org/dist/v24.18.0/node-v24.18.0-linux-arm64.tar.xz \
  58c9520501f6ae2b52d5b210444e24b9d0c029a58c5011b797bc1fe7105886f6 node.txz
rm -rf "$opt/node-v24.18.0-linux-arm64"; tar -xJf "$tmp/node.txz" -C "$opt"
for t in node npm npx; do ln -sf "$opt/node-v24.18.0-linux-arm64/bin/$t" "$bin/$t"; done
fetch https://github.com/microsoft/vscode-js-debug/releases/download/v1.95.3/js-debug-dap-v1.95.3.tar.gz \
  02d62834f9fe68fa20ef44dd0b77d7c2adf88d24c26d1172774fde31c38f6db1 js-debug.tgz
rm -rf "$opt/js-debug-1.95.3"; mkdir -p "$opt/js-debug-1.95.3"; tar -xzf "$tmp/js-debug.tgz" -C "$opt/js-debug-1.95.3"
PATH="$bin:$PATH" npm install -g --prefix "$opt/npm-tools" --ignore-scripts pyright@1.1.414 >/dev/null
for t in pyright pyright-langserver; do ln -sf "$opt/npm-tools/bin/$t" "$bin/$t"; done

# Gradle (services.gradle.org), the Kotlin Gradle plugin through one build of a tiny project,
# kotlin-debug-adapter (fwcd's release, the SHA-256 the Mac's install checks).
fetch https://services.gradle.org/distributions/gradle-8.10.2-bin.zip \
  31c55713e40233a8303827ceb42ca48a47267a0ad4bab9177123121e71524c26 gradle.zip
rm -rf "$opt/gradle-8.10.2"; unzip -q "$tmp/gradle.zip" -d "$opt"
ln -sf "$opt/gradle-8.10.2/bin/gradle" "$bin/gradle"
fetch https://github.com/fwcd/kotlin-debug-adapter/releases/download/0.4.4/adapter.zip \
  3874cbaded0fdb8229a381167895b0a6caf88b7adffabc690fcf5a6fb65d11b6 kda.zip
rm -rf "$opt/kotlin-debug-adapter-0.4.4"; mkdir -p "$opt/kotlin-debug-adapter-0.4.4"
unzip -q "$tmp/kda.zip" -d "$opt/kotlin-debug-adapter-0.4.4"
chmod +x "$opt/kotlin-debug-adapter-0.4.4/adapter/bin/kotlin-debug-adapter"
k="$tmp/kotlin"; mkdir -p "$k/src/main/kotlin/demo"
printf 'rootProject.name = "warm"\n' >"$k/settings.gradle.kts"
printf 'buildscript {\n    repositories { mavenCentral() }\n    dependencies { classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.0.21") }\n}\n\nplugins { application }\napply(plugin = "org.jetbrains.kotlin.jvm")\n\nrepositories { mavenCentral() }\n\napplication { mainClass.set("demo.MainKt") }\n' >"$k/build.gradle.kts"
printf 'package demo\n\nfun main() {\n    println("warm")\n}\n' >"$k/src/main/kotlin/demo/Main.kt"
(cd "$k" && JAVA_HOME=/usr/lib/jvm/java-21-openjdk-arm64 "$bin/gradle" --no-daemon -q build >/dev/null)

# ruby-lsp (rubygems.org) for the Ruby language server.
gem install --user-install --no-document ruby-lsp -v 0.26.11 >/dev/null
# ruby-lsp composes a bundle with bundler at its start: the self-test runs with the user's gems
# as GEM_HOME (a wrapper on the PATH would be found again by bundler and call itself).
ln -sf "$(ruby -e 'print Gem.user_dir')/bin/ruby-lsp" "$bin/ruby-lsp"
touch "$marker"
EOF

echo "== copying the tree into the VM"
ssh_config="$HOME/.lima/$VM/ssh.config"
rsync -a --delete --delete-excluded --exclude=/.git --exclude=/target --filter=':- .gitignore' \
  -e "ssh -F $ssh_config" ./ "lima-$VM:ferris-idea/"

echo "== building ferris-idea (debug)"
# Without incremental state: the debug build is 2.7 GB, and the 60 GiB disk filled up with it.
if ! in_vm bash -lc 'cd ~/ferris-idea && export CARGO_TARGET_DIR=$HOME/target CARGO_INCREMENTAL=0 && cargo build -p ferris-idea' \
  >"$OUT/build.log" 2>&1; then
  tail -40 "$OUT/build.log" >&2
  echo "error: build failed, log in $OUT/build.log" >&2
  exit 1
fi
[ "$build_only" = 1 ] && { echo "built; VM $VM keeps running"; exit 0; }

echo "== vulkan devices"
in_vm bash -lc 'vulkaninfo --summary 2>/dev/null | grep -E "deviceName|driverName" || echo "no vulkan device"'

echo "== self-test (timeout $TIMEOUT s)"
# Each step is named in the log (FERRIS_IDEA_SELFTEST_TRACE), so a failure can be put in its group.
envs="FERRIS_IDEA_SELFTEST_TRACE=${FERRIS_IDEA_SELFTEST_TRACE:-1}"
for v in FERRIS_IDEA_SELFTEST_ONLY FERRIS_IDEA_SELFTEST_SKIP FERRIS_IDEA_SELFTEST_DRIVER; do
  eval "val=\${$v:-}"
  [ -z "$val" ] && continue
  case "$val" in *[!A-Za-z0-9_,.-]*) echo "error: $v may hold names and commas only" >&2; exit 2 ;; esac
  envs="$envs $v=$val"
done
# The run is detached from the SSH session (a dropped session would end it) and leaves its log and
# exit code in the VM; the Mac waits for the code and copies the log.
in_vm bash -s "$TIMEOUT" $envs <<'EOF'
set -eu
limit=$1; shift
rm -f ~/selftest.log ~/selftest.rc
cd ~/ferris-idea
# The provisioned tools come first (clangd 21 before Ubuntu's 18, node 24 before the archive's 18).
export PATH="$HOME/.local/bin:$PATH"
GEM_HOME=$(ruby -e 'print Gem.user_dir' 2>/dev/null || true)
[ -n "$GEM_HOME" ] && export GEM_HOME && export PATH="$GEM_HOME/bin:$PATH"
setsid nohup bash -c 'env "$@" timeout -k 30 "$0" xvfb-run -a -s "-screen 0 1920x1080x24" \
  "$HOME/target/debug/ferris-idea" ui --selftest >"$HOME/selftest.log" 2>&1
  echo $? >"$HOME/selftest.rc"' "$limit" "$@" </dev/null >/dev/null 2>&1 &
EOF
waited=0
until in_vm bash -c 'test -s ~/selftest.rc' 2>/dev/null; do
  sleep 15; waited=$((waited + 15))
  if [ "$waited" -gt $((TIMEOUT + 120)) ]; then echo "error: no exit code after $waited s" >&2; break; fi
done
in_vm bash -c 'cat ~/selftest.log' >"$OUT/selftest.log" 2>/dev/null || true
rc=$(in_vm bash -c 'cat ~/selftest.rc 2>/dev/null' || true)
rc=${rc:-unknown}
echo "exit code $rc (124: timed out); log in $OUT/selftest.log"
grep -E '^[a-z].*(steps skipped|; skipped|need FERRIS_IDEA_SELFTEST_DRIVER)' "$OUT/selftest.log" | grep -v '^self-test' | sort -u || true
# The slowest groups (the self-test prints them slowest first).
grep -E '^self-test group ' "$OUT/selftest.log" | head -10 || true
result=$(grep '^{"checks"' "$OUT/selftest.log" | tail -1 || true)
if [ -z "$result" ]; then
  tail -40 "$OUT/selftest.log" >&2
  echo "error: no result line" >&2
  exit 1
fi
echo "$result" | tee "$OUT/result.json"
case "$result" in *'"failed":[]'*) [ "$rc" = 0 ] ;; *) exit 1 ;; esac
