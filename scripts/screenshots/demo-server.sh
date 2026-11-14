#!/usr/bin/env bash
# Starts or stops the demo server (core/talk-demo) for screenshot runs:
#   scripts/screenshots/demo-server.sh start [listen]   default 0.0.0.0:8099, waits until it answers
#   scripts/screenshots/demo-server.sh stop
# Every start resets the data: message times are relative to the start, so run screenshots
# during the day (the "today" messages lie a few hours back).
set -euo pipefail
cd "$(dirname "$0")/../.."
STATE="${XDG_RUNTIME_DIR:-/tmp}/wolkenfunk-demo"
mkdir -p "$STATE"
case "${1:-}" in
  start)
    LISTEN="${2:-0.0.0.0:8099}"
    "$0" stop >/dev/null 2>&1 || true
    command -v cargo >/dev/null || source "$HOME/.cargo/env"
    (cd core && cargo build -q --release -p talk-demo)
    nohup core/target/release/talk-demo --listen "$LISTEN" >"$STATE/log" 2>&1 &
    echo $! >"$STATE/pid"
    PORT="${LISTEN##*:}"
    for _ in $(seq 50); do
      curl -sf "http://127.0.0.1:$PORT/status.php" >/dev/null && { echo "demo server on $LISTEN (log: $STATE/log)"; exit 0; }
      sleep 0.2
    done
    echo "demo server did not start, see $STATE/log" >&2; exit 1 ;;
  stop)
    [[ -f "$STATE/pid" ]] && kill "$(cat "$STATE/pid")" 2>/dev/null && echo "demo server stopped"
    rm -f "$STATE/pid" ;;
  *) echo "usage: $0 start [listen] | stop" >&2; exit 2 ;;
esac
