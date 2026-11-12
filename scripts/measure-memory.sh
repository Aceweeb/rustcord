#!/usr/bin/env bash
# Measure Rustcord's real resource usage.
#
# Launches the release binary, samples /proc once per second, and prints RSS
# (current + kernel peak) and CPU per interval. Kills the app afterwards.
#
# Usage: scripts/measure-memory.sh [seconds] [--keep]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/release/rustcord"
DURATION="${1:-20}"
KEEP="${2:-}"

if [[ ! -x "$BIN" ]]; then
  echo "release binary missing; run: cargo build --release" >&2
  exit 1
fi

"$BIN" >/tmp/rustcord-measure.log 2>&1 &
APP=$!
trap 'kill "$APP" 2>/dev/null || true' EXIT

sleep 2
if ! kill -0 "$APP" 2>/dev/null; then
  echo "app exited during startup; log:" >&2
  tail -20 /tmp/rustcord-measure.log >&2
  exit 1
fi

prev_ticks=$(awk '{print $14+$15}' "/proc/$APP/stat")
printf '%-8s %-12s %-12s %-8s %-6s\n' "t(s)" "RSS" "peak(VmHWM)" "cpu%*" "thr"

for i in $(seq 1 "$DURATION"); do
  if ! kill -0 "$APP" 2>/dev/null; then
    echo "process exited at t=${i}s" >&2
    break
  fi

  read -r rss hwm threads < <(
    awk '/VmRSS/{rss=$2} /VmHWM/{hwm=$2} /^Threads/{thr=$2} END{print rss, hwm, thr}' \
      "/proc/$APP/status"
  )
  ticks=$(awk '{print $14+$15}' "/proc/$APP/stat")
  cpu=$(awk -v d="$((ticks - prev_ticks))" 'BEGIN{printf "%.1f", d}')
  prev_ticks=$ticks

  printf '%-8s %-12s %-12s %-8s %-6s\n' \
    "$i" "$((rss / 1024)) MB" "$((hwm / 1024)) MB" "$cpu" "$threads"
  sleep 1
done

echo
 echo "* cpu% = scheduler ticks consumed since the previous row. One tick is 10 ms,"
echo "  so 1 tick/s is about 1% of one core."
echo "Scope: this process only. Rustcord is a native egui/glow renderer with no"
echo "WebView, so there are no renderer subprocesses to add."
echo "Log: /tmp/rustcord-measure.log"

if [[ "$KEEP" == "--keep" ]]; then
  trap - EXIT
  echo "left running as pid $APP"
else
  kill "$APP" 2>/dev/null || true
  wait "$APP" 2>/dev/null || true
fi
