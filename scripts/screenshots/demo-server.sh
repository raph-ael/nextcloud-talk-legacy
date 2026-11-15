#!/usr/bin/env bash
# Starts or stops the demo server (core/talk-demo) for screenshot runs and tests:
#   scripts/screenshots/demo-server.sh start [listen] [fixtures]   default 0.0.0.0:8099, waits until it answers
#   scripts/screenshots/demo-server.sh stop [port]                 without a port: all instances
# Every start resets the data: message times are relative to the start, so run screenshots
# during the day (the "today" messages lie a few hours back).
#
# Several accounts (docs/accounts.md): a second instance with another user, one per port:
#   scripts/screenshots/demo-server.sh start 0.0.0.0:8098 core/talk-demo/fixtures-b
#   curl -d token=mira -d text=Hallo http://127.0.0.1:8098/demo/incoming    # somebody writes
set -euo pipefail
cd "$(dirname "$0")/../.."
STATE="${XDG_RUNTIME_DIR:-/tmp}/wolkenfunk-demo"
mkdir -p "$STATE"
stop_port() {
  local pid="$STATE/pid-$1"
  [[ -f "$pid" ]] && kill "$(cat "$pid")" 2>/dev/null && echo "demo server on port $1 stopped"
  rm -f "$pid"
}
case "${1:-}" in
  start)
    LISTEN="${2:-0.0.0.0:8099}"
    FIXTURES="${3:-core/talk-demo/fixtures}"
    PORT="${LISTEN##*:}"
    stop_port "$PORT" >/dev/null 2>&1 || true
    command -v cargo >/dev/null || source "$HOME/.cargo/env"
    (cd core && cargo build -q --release -p talk-demo)
    nohup core/target/release/talk-demo --listen "$LISTEN" --fixtures "$FIXTURES" >"$STATE/log-$PORT" 2>&1 &
    echo $! >"$STATE/pid-$PORT"
    for _ in $(seq 50); do
      curl -sf "http://127.0.0.1:$PORT/status.php" >/dev/null && { echo "demo server on $LISTEN (log: $STATE/log-$PORT)"; exit 0; }
      sleep 0.2
    done
    echo "demo server did not start, see $STATE/log-$PORT" >&2; exit 1 ;;
  stop)
    if [[ -n "${2:-}" ]]; then
      stop_port "$2" || true
    else
      for f in "$STATE"/pid-*; do
        [[ -f "$f" ]] && stop_port "${f##*pid-}" || true
      done
    fi ;;
  *) echo "usage: $0 start [listen] [fixtures] | stop [port]" >&2; exit 2 ;;
esac
