#!/bin/sh
# Window part of the performance budgets (SPEC-0260): starts the release build of `idecraft ui` on
# an empty project and measures from outside, the way OBS-0002 measured the reference (wall clock,
# `ps`, the public window list): time to the project window, time from the project window to idle
# (below 5 % of one core for 10 s, from CPU-time deltas), resident memory when idle and after
# `idle_minutes` without input. Prints one JSON line of metrics; scripts/ci-window.sh checks it.
# Opens a window, so it runs only where scripts/ci-window.sh allows windows.
#
#   perf-window.sh [idle_minutes]    default 10
set -eu
export LC_ALL=C
cd "$(dirname "$0")/.."
IDLE_MIN="${1:-10}"
BIN="${CARGO_TARGET_DIR:-target}/release/idecraft"
BENCH_DIR="${IDECRAFT_BENCH_DIR:-$HOME/.cache/idecraft-bench}"
WINS="$BENCH_DIR/bin/windows"
QUIET_CPU=5      # % of one core
QUIET_SECS=10    # consecutive seconds below QUIET_CPU
TIMEOUT=120      # s to wait for the window and for idle
[ -x "$BIN" ] || { echo "error: $BIN missing (cargo build --release -p idecraft)" >&2; exit 1; }
mkdir -p "$(dirname "$WINS")"
[ -x "$WINS" ] || swiftc -O -o "$WINS" scripts/windows.swift

work=$(mktemp -d)
pid=""
cleanup() {
  if [ -n "$pid" ] && kill "$pid" 2>/dev/null; then wait "$pid" 2>/dev/null || true; fi
  rm -rf "$work"
}
trap cleanup EXIT
mkdir -p "$work/empty" "$work/config"

now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
# CPU time of the process in milliseconds (`[[dd-]hh:]mm:ss.cc`).
cpu_ms() {
  ps -o time= -p "$1" 2>/dev/null | awk '{
    n = split($1, d, "-"); t = (n == 2) ? d[2] : d[1]; days = (n == 2) ? d[1] : 0
    k = split(t, p, ":"); s = 0; for (i = 1; i <= k; i++) s = s * 60 + p[i]
    printf "%d\n", (days * 86400 + s) * 1000 }'
}
rss_mib() { ps -o rss= -p "$1" 2>/dev/null | awk '{ printf "%d\n", $1 / 1024 }'; }

# macOS checks a newly linked executable at its first start; that is not the app's start time.
"$BIN" --version >/dev/null
# Own settings folder: no saved theme, keymap or MCP discovery file of the user's (SPEC-0221).
# Own process group, so signals for the app never reach this script.
set -m
T0=$(now_ms)
# Trusted for this run, so no trust question waits for an answer (SPEC-0278 behaviour 10).
IDECRAFT_TRUST_PROJECTS=all IDECRAFT_CONFIG_DIR="$work/config" "$BIN" ui "$work/empty" >"$work/out.log" 2>&1 &
pid=$!
set +m
t_win="" t_frame="" t_quiet="" quiet_since="" last_t="" last_cpu=""
while :; do
  t=$(( $(now_ms) - T0 ))
  [ "$t" -le $((TIMEOUT * 1000)) ] || { echo "error: no project window and idle within $TIMEOUT s" >&2; exit 1; }
  kill -0 "$pid" 2>/dev/null || { echo "error: idecraft ended early" >&2; cat "$work/out.log" >&2; exit 1; }
  set -- $("$WINS" "$pid")
  nwin=$1 size=$2
  [ -z "$t_win" ] && [ "$nwin" -gt 0 ] && t_win=$t
  # project window: at least 1000 px wide, as for the reference
  [ -z "$t_frame" ] && [ "${size%%x*}" -ge 1000 ] && t_frame=$t
  cpu=$(cpu_ms "$pid")
  if [ -n "$t_frame" ] && [ -n "$last_t" ] && [ "$t" -gt "$last_t" ]; then
    # share of one core since the last sample
    if [ $(( (cpu - last_cpu) * 100 )) -lt $(( QUIET_CPU * (t - last_t) )) ]; then
      [ -n "$quiet_since" ] || quiet_since=$last_t
      if [ $((t - quiet_since)) -ge $((QUIET_SECS * 1000)) ]; then t_quiet=$quiet_since; break; fi
    else
      quiet_since=""
    fi
  fi
  last_t=$t last_cpu=$cpu
  if [ -n "$t_frame" ]; then sleep 1; else sleep 0.1; fi
done
rss_start=$(rss_mib "$pid")
cpu_start=$(cpu_ms "$pid")
sleep $((IDLE_MIN * 60))
rss_idle=$(rss_mib "$pid")
cpu_idle=$(( $(cpu_ms "$pid") - cpu_start ))
printf '{"start_first_window_ms":%d,"start_window_ms":%d,"start_idle_s":%s,"start_rss_mib":%d,"idle_minutes":%d,"idle_rss_mib":%d,"idle_growth_mib":%d,"idle_cpu_pct":%s}\n' \
  "$t_win" "$t_frame" "$(awk -v a="$t_quiet" -v b="$t_frame" 'BEGIN { d = (a - b) / 1000; printf "%.1f", (d < 0 ? 0 : d) }')" \
  "$rss_start" "$IDLE_MIN" "$rss_idle" "$((rss_idle - rss_start))" \
  "$(awk -v c="$cpu_idle" -v m="$IDLE_MIN" 'BEGIN { printf "%.2f", (m > 0 ? c / (m * 600) : 0) }')"
