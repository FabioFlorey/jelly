#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=./config.sh
source "$ROOT/scripts/config.sh"
BUILD_DIR="$CONFIG_BUILD_ROOT"
MODE="${JELLY_HOSTING_MODE:-local}"
ADDR="${JELLY_MCP_ADDR:-127.0.0.1:8787}"
MCP_BIN="${JELLY_MCP_BIN:-$BUILD_DIR/release/jelly-mcp}"

resolve_cloudflared() {
  if [[ -n "${JELLY_CLOUDFLARED_BIN:-}" ]]; then
    printf '%s\n' "$JELLY_CLOUDFLARED_BIN"
  elif command -v cloudflared >/dev/null 2>&1; then
    command -v cloudflared
  elif [[ -x "$HOME/.config/pilink/bin/cloudflared" ]]; then
    printf '%s\n' "$HOME/.config/pilink/bin/cloudflared"
  else
    echo "cloudflared not found; install it or set JELLY_CLOUDFLARED_BIN" >&2
    return 1
  fi
}

case "$MODE" in
  local)
    exec "$MCP_BIN"
    ;;

  cloudflare-fixed)
    if [[ -z "${JELLY_PUBLIC_URL:-}" ]]; then
      echo "JELLY_PUBLIC_URL is required for cloudflare-fixed hosting" >&2
      exit 2
    fi
    exec "$MCP_BIN"
    ;;

  nip-io)
    exec "$ROOT/scripts/run-nip-io.sh"
    ;;

  quick-tunnel)
    cloudflared="$(resolve_cloudflared)"
    log="$(mktemp)"
    tunnel_pid=""
    mcp_pid=""

    cleanup() {
      [[ -n "$mcp_pid" ]] && kill "$mcp_pid" >/dev/null 2>&1 || true
      [[ -n "$tunnel_pid" ]] && kill "$tunnel_pid" >/dev/null 2>&1 || true
      rm -f "$log"
    }
    trap cleanup EXIT INT TERM

    "$cloudflared" tunnel --url "http://$ADDR" > >(tee -a "$log") 2> >(tee -a "$log" >&2) &
    tunnel_pid=$!

    public_url=""
    for _ in $(seq 1 150); do
      if ! kill -0 "$tunnel_pid" >/dev/null 2>&1; then
        echo "cloudflared exited before publishing a Quick Tunnel URL" >&2
        exit 1
      fi
      public_url="$(grep -Eo 'https://[-a-z0-9]+\.trycloudflare\.com' "$log" | head -1 || true)"
      [[ -n "$public_url" ]] && break
      sleep 0.2
    done

    if [[ -z "$public_url" ]]; then
      echo "timed out waiting for Cloudflare Quick Tunnel URL" >&2
      exit 1
    fi

    export JELLY_PUBLIC_URL="$public_url"
    export JELLY_OAUTH_PUBLIC_CHATGPT_DCR="${JELLY_OAUTH_PUBLIC_CHATGPT_DCR:-true}"
    printf 'jelly Quick Tunnel: %s/mcp\n' "$public_url"

    "$MCP_BIN" &
    mcp_pid=$!

    while true; do
      if ! kill -0 "$tunnel_pid" >/dev/null 2>&1; then
        wait "$tunnel_pid" || true
        echo "cloudflared stopped; stopping jelly MCP" >&2
        exit 1
      fi
      if ! kill -0 "$mcp_pid" >/dev/null 2>&1; then
        wait "$mcp_pid" || true
        echo "jelly MCP stopped; stopping cloudflared" >&2
        exit 1
      fi
      sleep 1
    done
    ;;

  *)
    echo "JELLY_HOSTING_MODE must be local, quick-tunnel, nip-io, or cloudflare-fixed" >&2
    exit 2
    ;;
esac
