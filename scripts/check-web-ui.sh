#!/usr/bin/env bash
# Isolated HTTP + headless Chromium smoke test of the shared Jelly web design.
# Does not restart the installed Jelly service or modify its .env.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/config.sh
source "$ROOT/scripts/config.sh"
cd "$ROOT"
BIN="${JELLY_UI_TEST_BIN:-$CONFIG_BUILD_ROOT/debug/jelly-mcp}"
if [[ ! -x "$BIN" ]]; then
  cargo build --quiet --bin jelly-mcp
fi
CHROMIUM="$(command -v chromium || command -v chromium-browser || true)"
[[ -n "$CHROMIUM" ]] || { echo 'chromium is required for web UI screenshots' >&2; exit 2; }
command -v curl >/dev/null || { echo 'curl is required' >&2; exit 2; }

DIR="$(mktemp -d)"
SERVER_PID=""
cleanup() {
  if [[ -n "$SERVER_PID" ]]; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  if [[ -n "${JELLY_UI_SCREENSHOTS_DIR:-}" ]]; then
    mkdir -p "$JELLY_UI_SCREENSHOTS_DIR"
    cp "$DIR"/*.png "$JELLY_UI_SCREENSHOTS_DIR/" 2>/dev/null || true
  fi
  rm -rf "$DIR"
}
trap cleanup EXIT

read -r MCP_PORT ADMIN_PORT < <(python3 - <<'PY'
import socket
sockets = [socket.socket(), socket.socket()]
try:
    for sock in sockets:
        sock.bind(('127.0.0.1', 0))
    print(*(sock.getsockname()[1] for sock in sockets))
finally:
    for sock in sockets:
        sock.close()
PY
)
export JELLY_MCP_ADDR="127.0.0.1:$MCP_PORT"
export JELLY_MCP_ADMIN_ADDR="127.0.0.1:$ADMIN_PORT"
export JELLY_PUBLIC_URL="https://jelly.invalid"
export JELLY_OAUTH_CONSENT_MODE=paired
export JELLY_OAUTH_PUBLIC_CHATGPT_DCR=true
export JELLY_MCP_TOKEN=01234567890123456789012345678901
export JELLY_BOOTSTRAP_SECRET=abcdef0123456789abcdef0123456789
"$BIN" >"$DIR/server.log" 2>&1 &
SERVER_PID=$!
BASE="http://127.0.0.1:$MCP_PORT"
ADMIN="http://127.0.0.1:$ADMIN_PORT"
for _ in {1..60}; do
  if curl -fsS --max-time 1 "$BASE/health" > /dev/null 2>&1; then break; fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then cat "$DIR/server.log" >&2; exit 1; fi
  sleep 0.1
done
curl -fsS "$BASE/health" >/dev/null
curl -fsS "$BASE/ready" >/dev/null
curl -fsS "$BASE/connections" > "$DIR/connections.html"
grep -q '/brand/jelly.css' "$DIR/connections.html"
curl -fsS "$ADMIN/connect" > "$DIR/connect.html"
grep -q 'must be started by Jelly' "$DIR/connect.html"
curl -fsS "$ADMIN/brand/jelly.css" > "$DIR/jelly.css"
grep -q 'DynaPuff' "$DIR/jelly.css"
curl -fsS "$ADMIN/brand/full-logo.png" >/dev/null
[[ "$(curl -s -o /dev/null -w '%{http_code}' "$BASE/status.json")" == 403 ]]
[[ "$(curl -s -o /dev/null -w '%{http_code}' "$BASE/dashboard")" == 403 ]]
curl -fsS -H "Authorization: Bearer $JELLY_BOOTSTRAP_SECRET" "$BASE/status.json" >/dev/null

for target in "connections:$BASE/connections" "connect:$ADMIN/connect"; do
  name="${target%%:*}"
  url="${target#*:}"
  "$CHROMIUM" --headless --no-sandbox --disable-gpu --disable-dev-shm-usage \
    --disable-background-networking --user-data-dir="$DIR/profile-$name" \
    --window-size=1100,850 --screenshot="$DIR/$name.png" "$url" \
    >"$DIR/chromium-$name.log" 2>&1
  [[ -s "$DIR/$name.png" ]] || { cat "$DIR/chromium-$name.log" >&2; exit 1; }
done
printf 'Jelly web UI smoke passed: branding, authorization and Chromium renders.\n'
if [[ -n "${JELLY_UI_SCREENSHOTS_DIR:-}" ]]; then
  printf 'Screenshots copied to %s\n' "$JELLY_UI_SCREENSHOTS_DIR"
fi
