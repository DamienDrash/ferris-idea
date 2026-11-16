#!/bin/sh
# Fuzz targets of M8.2 (docs/testing/HARDENING.md).
#
#   fuzz.sh smoke [SECONDS]                 every target with the stable mutation driver
#                                           (default 3 s each; scripts/ci-local.sh runs this)
#   fuzz.sh libfuzzer [SECONDS] [TARGET…]   coverage-guided with cargo fuzz on nightly
#                                           (default 300 s each, all targets, low priority)
#
# Crashing inputs: fuzz-crashes/ (smoke) or fuzz/artifacts/<target>/ (libFuzzer). Replay one with
#   cargo run -p idecraft-hardening --bin idecraft-fuzz -- replay TARGET FILE
set -eu
cd "$(dirname "$0")/.."
mode="${1:-smoke}"
[ $# -gt 0 ] && shift

case "$mode" in
  smoke)
    secs="${1:-3}"
    cargo build -q -p idecraft-hardening --bin idecraft-fuzz
    "${CARGO_TARGET_DIR:-target}/debug/idecraft-fuzz" run --secs "$secs" --corpus fuzz/corpus
    ;;
  libfuzzer)
    secs="${1:-300}"
    [ $# -gt 0 ] && shift
    if ! cargo +nightly fuzz --version >/dev/null 2>&1; then
      echo "needs the nightly toolchain and cargo-fuzz: rustup toolchain install nightly; cargo install cargo-fuzz" >&2
      exit 2
    fi
    cargo build -q -p idecraft-hardening --bin idecraft-fuzz
    targets="$*"
    if [ -z "$targets" ]; then
      targets=$("${CARGO_TARGET_DIR:-target}/debug/idecraft-fuzz" list | awk '{print $1}')
    fi
    # Seeds into the (git-ignored) working corpus; libFuzzer adds what it finds.
    "${CARGO_TARGET_DIR:-target}/debug/idecraft-fuzz" seeds fuzz/corpus
    cd fuzz
    failed=0
    for t in $targets; do
      printf '\n== %s (%s s)\n' "$t" "$secs"
      # No sanitizer: the targets are safe Rust, so panics, aborts, timeouts and huge allocations
      # are what there is to find; -s address works too, just slower.
      if ! nice -n 19 cargo +nightly fuzz run -s none -O "$t" "corpus/$t" -- \
          -max_total_time="$secs" -timeout=30 -rss_limit_mb=4096 -malloc_limit_mb=1024 \
          -print_final_stats=1 2>&1 | grep -E '^(stat::number_of_executed_units|stat::peak_rss_mb|#[0-9]+ +DONE|==[0-9]+== ERROR|SUMMARY|.*panicked at|artifact_prefix|Test unit written)'; then
        :
      fi
      if [ -n "$(find "artifacts/$t" \( -name 'crash-*' -o -name 'oom-*' -o -name 'timeout-*' \) 2>/dev/null)" ]; then
        echo "FINDINGS in fuzz/artifacts/$t"
        failed=1
      fi
    done
    exit "$failed"
    ;;
  *)
    sed -n '2,10p' "$0"
    exit 2
    ;;
esac
