#!/bin/sh
# Window tests on the Mac: the workbench self-test (`ferris-idea ui --selftest`), the M3.4 GPUI
# spike measurements of ADR-0003 (scroll, typing latency, two-window layout) and the window part of
# the performance budgets (SPEC-0260: start, idle, memory; scripts/perf-window.sh). They open
# windows and send keys, so they must never run while someone works at the Mac.
#
#   ci-window.sh            nightly mode: runs only between 00:00 and 06:59 local time with the Mac
#                           idle for at least 10 minutes; otherwise it skips and exits 0
#   ci-window.sh --force    on request (workflow_dispatch selftest, or by hand): no time/idle guard
#   ci-window.sh --check    only print whether nightly mode would run now
#
# PERF_IDLE_MINUTES (default 10) sets how long the memory is watched without input.
#
# macOS sends no frames to a locked screen, so a locked Mac skips (nightly) or fails (--force).
set -eu
# Variables under the former name (IDECRAFT_*) still count while today's is not set. rebrand:keep
for former in $(env | sed -n 's/^IDECRAFT_\([A-Za-z0-9_]*\)=.*/\1/p'); do # rebrand:keep
  eval "[ -n \"\${FERRIS_IDEA_$former+x}\" ] || { FERRIS_IDEA_$former=\$IDECRAFT_$former; export FERRIS_IDEA_$former; }" # rebrand:keep
done
export FERRIS_IDEA_NO_TAKEOVER=1 # the programs this script starts never move the user's settings folder (SPEC-0356)
cd "$(dirname "$0")/.."

mode="${1:-nightly}"
case "$mode" in nightly | --force | --check) ;; *) echo "usage: ci-window.sh [--force | --check]" >&2; exit 2 ;; esac

notice() {
  echo "$*"
  [ -n "${GITHUB_ACTIONS:-}" ] && echo "::warning title=Window tests::$*"
  return 0
}

locked() {
  ioreg -n Root -d1 -a | plutil -extract IOConsoleUsers.0.CGSSessionScreenIsLocked raw - 2>/dev/null | grep -q true
}

# Reason why nightly mode must not open windows now, or nothing.
day_guard() {
  hour=$(date +%H)
  idle=$(ioreg -c IOHIDSystem -d4 | awk '/HIDIdleTime/ { print int($NF / 1000000000); exit }')
  if [ "${hour#0}" -ge 7 ] 2>/dev/null; then
    echo "local time $(date +%H:%M) is outside 00:00-06:59"
  elif [ "${idle:-0}" -lt 600 ]; then
    echo "the Mac was used ${idle:-0} s ago (needs 600 s idle)"
  fi
}

if [ "$mode" != "--force" ]; then
  reason=$(day_guard)
  if [ "$mode" = "--check" ]; then
    echo "${reason:-would run}"; exit 0
  fi
  if [ -n "$reason" ]; then
    notice "skipped: $reason"; exit 0
  fi
fi
if locked; then
  [ "$mode" = "--force" ] && { echo "error: the screen is locked; macOS sends no frames" >&2; exit 1; }
  notice "skipped: the screen is locked; macOS sends no frames"; exit 0
fi

out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
failed=0

# Runs a window command with a watchdog; its JSON result lines (one per window) land in
# $out/$name.json.
run() {
  name=$1 secs=$2; shift 2
  printf '\n== %s\n' "$name"
  "$@" >"$out/$name.log" &
  pid=$!
  (sleep "$secs" && kill "$pid" 2>/dev/null && echo "error: $name timed out after $secs s" >&2) &
  dog=$!
  rc=0; wait "$pid" || rc=$?
  kill "$dog" 2>/dev/null || true
  grep '^{' "$out/$name.log" | tee "$out/$name.json"
  if [ "$rc" -ne 0 ]; then echo "error: $name exited with $rc" >&2; failed=1; fi
}

# Checks one jq condition on every result line; prints the condition when it does not hold.
expect() {
  if ! jq -e -s "length > 0 and all(.[]; $2)" "$out/$1.json" >/dev/null 2>&1; then
    echo "error: $1: expected $2" >&2; failed=1
  fi
}

cargo build -q -p ferris-idea
cargo build -q --release -p ferris-idea -p ferris-idea-bench
run selftest 900 "${CARGO_TARGET_DIR:-target}/debug/ferris-idea" ui --selftest
expect selftest '.failed == []'
# ADR-0003 criteria 1, 2 and 4 with release builds; 50 fps leaves room for a 60 Hz display.
run spike-scroll 300 "${CARGO_TARGET_DIR:-target}/release/ferris-idea" spike-scroll
expect spike-scroll '.frames >= 900 and .fps >= 50'
run spike-editor 300 "${CARGO_TARGET_DIR:-target}/release/ferris-idea" spike-editor
expect spike-editor '.keystrokes >= 200 and .latency_ms.p95 < 16'
run spike-layout 300 "${CARGO_TARGET_DIR:-target}/release/ferris-idea" spike-layout .
expect spike-layout '.frames >= 600 and .fps >= 50 and .table_rows == 100000'
# SPEC-0260: start, idle and memory of the release build, and the budgets of every window metric
# (typing latency from spike-editor); the result joins the history next to the headless runs.
run perf-start $(( ${PERF_IDLE_MINUTES:-10} * 60 + 300 )) scripts/perf-window.sh "${PERF_IDLE_MINUTES:-10}"
jq -c '{typing_frame_ms_p50: .latency_ms.p50, typing_frame_ms_p95: .latency_ms.p95,
        typing_frame_ms_p99: .latency_ms.p99}' "$out/spike-editor.json" >"$out/perf-typing.json" || true
bench_dir="${FERRIS_IDEA_BENCH_DIR:-$HOME/.cache/ferris-idea-bench}"
mkdir -p "$bench_dir"
jq -c -s --arg commit "$(git rev-parse --short HEAD)" --arg date "$(date +%Y-%m-%dT%H:%M:%S%z)" \
  --arg load "$(sysctl -n vm.loadavg | awk '{ print $2 }')" \
  '{date: $date, commit: $commit, load_avg: ($load | tonumber), mode: "window", metrics: add}' \
  "$out/perf-start.json" "$out/perf-typing.json" | tee -a "$bench_dir/history.jsonl" >"$out/perf.json" || true
printf '\n== performance budgets (window)\n'
"${CARGO_TARGET_DIR:-target}/release/ferris-idea-bench" check window "$out/perf.json" | tee "$out/perf-budgets.txt" || failed=1

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    echo "### Window tests (macOS)"
    for name in selftest spike-scroll spike-editor spike-layout perf-start; do
      printf -- '- `%s`: `%s`\n' "$name" "$(tr '\n' ' ' <"$out/$name.json")"
    done
    echo '```'; cat "$out/perf-budgets.txt" 2>/dev/null; echo '```'
  } >>"$GITHUB_STEP_SUMMARY"
fi
[ "$failed" -eq 0 ] && printf '\n== window tests passed\n'
exit "$failed"
