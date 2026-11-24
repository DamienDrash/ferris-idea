#!/bin/sh
# Runs inside the Debian container started by build-backends.sh: installs the cross toolchains from
# Debian packages and builds `ferris-idea-serve` for each of $TARGETS. The output lands in
# $OUT/<target>/ (default: dist/backends in the mounted source folder). With CHECK_ONLY=1 it runs
# `cargo check` only, for machines too small to link a release build.
set -eu

OUT=${OUT:-dist/backends}
apt-get update -q >/dev/null
apt-get install -y -q --no-install-recommends \
  gcc-aarch64-linux-gnu g++-aarch64-linux-gnu gcc-x86-64-linux-gnu g++-x86-64-linux-gnu \
  gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 cmake lld >/dev/null

export CARGO_TARGET_DIR=/tmp/backends-target
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++
export CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++

for t in $TARGETS; do
  rustup target add "$t" >/dev/null
  if [ "$t" = aarch64-pc-windows-gnullvm ]; then
    # llvm-mingw: a release of mstorsjo/llvm-mingw (Apache-2.0 with LLVM exception).
    if [ ! -d /opt/llvm-mingw ]; then
      arch=$(uname -m)
      url="https://github.com/mstorsjo/llvm-mingw/releases/download/$LLVM_MINGW_VERSION/llvm-mingw-$LLVM_MINGW_VERSION-ucrt-ubuntu-22.04-$arch.tar.xz"
      echo "fetching $url"
      apt-get install -y -q --no-install-recommends curl xz-utils >/dev/null
      curl -fsSL "$url" -o /tmp/llvm-mingw.tar.xz
      mkdir /opt/llvm-mingw
      tar -xJf /tmp/llvm-mingw.tar.xz -C /opt/llvm-mingw --strip-components=1
    fi
    PATH=/opt/llvm-mingw/bin:$PATH
    export CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER=aarch64-w64-mingw32-clang
    export CC_aarch64_pc_windows_gnullvm=aarch64-w64-mingw32-clang
    export CXX_aarch64_pc_windows_gnullvm=aarch64-w64-mingw32-clang++
  fi
  echo "== $t"
  # The database stack below the host's MCP tools does not build for Windows yet (SPEC-0321).
  case "$t" in
    *windows*) features=--no-default-features ;;
    *) features= ;;
  esac
  if [ "${CHECK_ONLY:-0}" = 1 ]; then
    cargo check --locked -p ferris-idea-serve $features --target "$t"
    continue
  fi
  cargo build --locked --release -p ferris-idea-serve $features --target "$t"
  mkdir -p "$OUT/$t"
  case "$t" in
    *windows*) cp "$CARGO_TARGET_DIR/$t/release/ferris-idea-serve.exe" "$OUT/$t/" ;;
    *) cp "$CARGO_TARGET_DIR/$t/release/ferris-idea-serve" "$OUT/$t/" ;;
  esac
done
