#!/bin/sh
# Long-session soak test of M8.2 (docs/testing/HARDENING.md): drives `ferris-idea mcp` on a scratch
# workspace and fails on memory or file-descriptor growth, leftover child processes or a request
# without an answer.
#
#   soak.sh [MINUTES] [ferris-idea-soak options…]    default 2 minutes (scripts/ci-local.sh)
#   soak.sh 120 --csv soak.csv                     the nightly run
set -eu
cd "$(dirname "$0")/.."
minutes="${1:-2}"
[ $# -gt 0 ] && shift
cargo build -q -p ferris-idea -p ferris-idea-hardening --bin ferris-idea --bin ferris-idea-soak
bin="${CARGO_TARGET_DIR:-target}/debug"
exec nice -n 10 "$bin/ferris-idea-soak" --server "$bin/ferris-idea" --minutes "$minutes" "$@"
