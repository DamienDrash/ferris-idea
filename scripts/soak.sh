#!/bin/sh
# Long-session soak test of M8.2 (docs/testing/HARDENING.md): drives `idecraft mcp` on a scratch
# workspace and fails on memory or file-descriptor growth, leftover child processes or a request
# without an answer.
#
#   soak.sh [MINUTES] [idecraft-soak options…]    default 2 minutes (scripts/ci-local.sh)
#   soak.sh 120 --csv soak.csv                     the nightly run
set -eu
cd "$(dirname "$0")/.."
minutes="${1:-2}"
[ $# -gt 0 ] && shift
cargo build -q -p idecraft -p idecraft-hardening --bin idecraft --bin idecraft-soak
bin="${CARGO_TARGET_DIR:-target}/debug"
exec nice -n 10 "$bin/idecraft-soak" --server "$bin/idecraft" --minutes "$minutes" "$@"
