#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/config.sh
source "$ROOT/scripts/config.sh"
BUILD_DIR="$CONFIG_BUILD_ROOT"
RUNTIME="$CONFIG_RUNTIME_ROOT"

# Fail closed before stopping services or removing any files.
validate_cleanup_target() {
  local original="$1" resolved
  [[ -n "$original" && "$original" == /* ]] || { echo "unsafe cleanup path: $original" >&2; return 2; }
  resolved="$(realpath -m -- "$original")" || return 2
  case "$resolved" in
    /|/data|/home|/tmp|"$ROOT"|"$HOME")
      echo "refusing broad cleanup path: $resolved" >&2; return 2 ;;
  esac
  [[ "$resolved" == "$original" && "$resolved" != "$ROOT"/* ]] || {
    echo "refusing non-canonical or source-tree cleanup target: $original" >&2; return 2;
  }
  case "$resolved" in
    */jelly-runtime|*/.jelly-build|*/jelly-build|*/jelly-target) ;;
    *) echo "refusing non-Jelly cleanup target: $resolved" >&2; return 2 ;;
  esac
}
validate_cleanup_target "$RUNTIME"
validate_cleanup_target "$BUILD_DIR"

systemctl --user stop jelly-cloudflared.service >/dev/null 2>&1 || true
systemctl --user stop jelly-mcp.service >/dev/null 2>&1 || true
systemctl --user stop jelly-browser.service >/dev/null 2>&1 || true

if [[ -f "$RUNTIME/network/capture.pid" ]]; then
  pid="$(cat "$RUNTIME/network/capture.pid" 2>/dev/null || true)"
  # PID files can outlive their process. Never signal an unrelated reused PID.
  if [[ "$pid" =~ ^[0-9]+$ ]] && [[ -r "/proc/$pid/cmdline" ]] \
      && tr '\0' ' ' < "/proc/$pid/cmdline" | grep -Fq 'agent-inspect-network __capture'; then
    kill "$pid" >/dev/null 2>&1 || true
  fi
fi

rm -rf -- "$RUNTIME"
# Remove pre-layout legacy runtime/build directories if they still exist.
rm -rf -- "$ROOT/temp" "$ROOT/logs" "$ROOT/target" "$BUILD_DIR"

if [[ "${1:-}" == "--build" ]]; then
  (cd "$ROOT" && cargo clean)
fi

printf 'jelly runtime cleaned%s.\n' "$([[ "${1:-}" == "--build" ]] && printf ' (including Cargo build output)' || true)"
