#!/bin/sh
# Builds the headless backend `ferris-idea-serve` for every platform of SPEC-0321 into
# dist/backends/<target>/ and writes dist/backends/SHA256SUMS.
#
#   build-backends.sh [--mac-only | --linux-only] [<target> ...]
#
# macOS targets build natively (Xcode's clang handles both architectures). Linux and Windows
# targets need a C toolchain for the grammar crates, so they build inside a Debian container with
# the engine from `containers.sh` of the reference environment (set DOCKER_HOST, or run
# `containers.sh start` and `eval "$(containers.sh env)"`). No account or payment is involved.
#
# A target that was only compiled is not the same as one that ran: the project owner's status file records what ran
# where.
set -eu
cd "$(dirname "$0")/.."

ALL_MAC="aarch64-apple-darwin x86_64-apple-darwin"
ALL_DOCKER="aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu x86_64-pc-windows-gnu"
LLVM_MINGW_VERSION=20251104
OUT=dist/backends
RUST_IMAGE=rust:1-bookworm

mac=1; docker=1
targets=""
for a in "$@"; do
  case "$a" in
    --mac-only) docker=0 ;;
    --linux-only) mac=0 ;;
    *) targets="$targets $a" ;;
  esac
done

mkdir -p "$OUT"

want() { [ -z "$targets" ] || case " $targets " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

if [ "$mac" = 1 ] && [ "$(uname -s)" = Darwin ]; then
  for t in $ALL_MAC; do
    want "$t" || continue
    echo "== $t (native)"
    rustup target add "$t" >/dev/null 2>&1 || true
    cargo build --release -p ferris-idea-serve --target "$t"
    mkdir -p "$OUT/$t"
    cp "target/$t/release/ferris-idea-serve" "$OUT/$t/ferris-idea-serve"
  done
fi

if [ "$docker" = 1 ]; then
  need=""
  for t in $ALL_DOCKER aarch64-pc-windows-gnullvm; do
    if want "$t"; then need="$need $t"; fi
  done
  if [ -n "$need" ]; then
    if ! command -v docker >/dev/null 2>&1; then
      echo "docker not found: run containers.sh tools and start (reference environment), or use --mac-only" >&2
      exit 2
    fi
    echo "== linux, windows in $RUST_IMAGE:$need"
    docker run --rm \
      -v "$PWD:/src" -v ferris-idea-cargo-registry:/usr/local/cargo/registry \
      -e TARGETS="$need" -e LLVM_MINGW_VERSION="$LLVM_MINGW_VERSION" -w /src \
      "$RUST_IMAGE" sh /src/scripts/build-backends-inner.sh
  fi
fi

( cd "$OUT"; find . -type f -name 'ferris-idea-serve*' | sort | xargs shasum -a 256 > SHA256SUMS; cat SHA256SUMS )
