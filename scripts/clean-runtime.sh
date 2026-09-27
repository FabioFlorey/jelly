#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$(cd "$ROOT/.." && pwd)/.jelly-build"
RUNTIME="/data/jelly-runtime"

systemctl --user stop jelly-cloudflared.service >/dev/null 2>&1 || true
systemctl --user stop jelly-mcp.service >/dev/null 2>&1 || true
systemctl --user stop jelly-browser.service >/dev/null 2>&1 || true

if [[ -f "$RUNTIME/network/capture.pid" ]]; then
  pid="$(cat "$RUNTIME/network/capture.pid" 2>/dev/null || true)"
  [[ -n "$pid" ]] && kill "$pid" >/dev/null 2>&1 || true
fi

rm -rf "$RUNTIME"
# Remove pre-layout legacy runtime/build directories if they still exist.
rm -rf "$ROOT/temp" "$ROOT/logs" "$ROOT/target" "$BUILD_DIR"

if [[ "${1:-}" == "--build" ]]; then
  (cd "$ROOT" && cargo clean)
fi

printf 'jelly runtime cleaned%s.\n' "$([[ "${1:-}" == "--build" ]] && printf ' (including Cargo build output)' || true)"
