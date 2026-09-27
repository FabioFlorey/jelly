#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
USER_UNITS="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
ENV_FILE="$ROOT/.env"

if [[ ! -f "$ENV_FILE" ]]; then
  echo "missing $ENV_FILE; copy .env.example and configure MCP secrets first" >&2
  exit 2
fi
chmod 600 "$ENV_FILE"

set -a
# shellcheck disable=SC1090
source "$ENV_FILE"
set +a

mcp_token="${JELLY_MCP_TOKEN:-}"
bootstrap_secret="${JELLY_BOOTSTRAP_SECRET:-}"
consent_mode="${JELLY_OAUTH_CONSENT_MODE:-browser}"
hosting_mode="${JELLY_HOSTING_MODE:-local}"

if [[ ${#mcp_token} -lt 32 ]]; then
  echo "JELLY_MCP_TOKEN must be set to at least 32 bytes" >&2
  exit 2
fi
if [[ ${#bootstrap_secret} -lt 32 ]]; then
  echo "JELLY_BOOTSTRAP_SECRET must be set to at least 32 bytes" >&2
  exit 2
fi
case "$consent_mode" in
  browser)
    oauth_password="${JELLY_OAUTH_PASSWORD:-}"
    if [[ ${#oauth_password} -lt 16 ]]; then
      echo "JELLY_OAUTH_PASSWORD must be at least 16 bytes in browser consent mode" >&2
      exit 2
    fi
    ;;
  paired) ;;
  *)
    echo "JELLY_OAUTH_CONSENT_MODE must be browser or paired" >&2
    exit 2
    ;;
esac
case "$hosting_mode" in
  local|quick-tunnel|nip-io|cloudflare-fixed) ;;
  *)
    echo "JELLY_HOSTING_MODE must be local, quick-tunnel, nip-io, or cloudflare-fixed" >&2
    exit 2
    ;;
esac

if [[ "$hosting_mode" == "cloudflare-fixed" ]]; then
  if [[ -z "${JELLY_PUBLIC_URL:-}" ]]; then
    echo "JELLY_PUBLIC_URL is required for cloudflare-fixed hosting" >&2
    exit 2
  fi
  if [[ -z "${JELLY_CLOUDFLARE_TUNNEL_TOKEN:-}" ]]; then
    echo "JELLY_CLOUDFLARE_TUNNEL_TOKEN is required for cloudflare-fixed hosting" >&2
    exit 2
  fi
fi

if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
  C_RESET=$'\033[0m'; C_BOLD=$'\033[1m'; C_DIM=$'\033[2m'
  C_GREEN=$'\033[32m'; C_RED=$'\033[31m'; C_HONEY=$'\033[38;2;255;193;7m'
else
  C_RESET=''; C_BOLD=''; C_DIM=''; C_GREEN=''; C_RED=''; C_HONEY=''
fi

cd "$ROOT"
BUILD_DIR="$(cd "$ROOT/.." && pwd)/.jelly-build"
MCP_BIN="$BUILD_DIR/release/jelly-mcp"
BUILD_STAMP="$BUILD_DIR/release/.jelly-build-stamp"
needs_build=false

if [[ ! -x "$MCP_BIN" || ! -f "$BUILD_STAMP" ]]; then
  needs_build=true
elif find \
  "$ROOT/src" \
  "$ROOT/Cargo.toml" \
  "$ROOT/Cargo.lock" \
  "$ROOT/rust-toolchain.toml" \
  "$ROOT/scripts/build_tool_index.rs" \
  "$ROOT/assets/full-logo.png" \
  "$ROOT/assets/favicon.png" \
  -type f -newer "$BUILD_STAMP" -print -quit 2>/dev/null | grep -q .; then
  needs_build=true
else
  for source in "$ROOT"/src/bin/*.rs; do
    bin="$BUILD_DIR/release/$(basename "${source%.rs}")"
    if [[ ! -x "$bin" ]]; then
      needs_build=true
      break
    fi
  done
fi

if [[ "$needs_build" == "true" ]]; then
  printf '\n%b◇  Building Jelly%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
  if ! cargo build --quiet --release --bins; then
    printf '%b✕  Build failed.%b\n' "$C_RED$C_BOLD" "$C_RESET" >&2
    exit 1
  fi
  touch "$BUILD_STAMP"
  printf '%b✓%b  Release binaries ready.\n' "$C_GREEN$C_BOLD" "$C_RESET"
else
  printf '\n%b✓%b  Release binaries are current; build skipped.\n' "$C_GREEN$C_BOLD" "$C_RESET"
fi

mkdir -p "$USER_UNITS"
root_escaped="${ROOT//\\/\\\\}"
root_escaped="${root_escaped//&/\\&}"
sed "s|/data/jelly|$root_escaped|g" "$ROOT/systemd/jelly-mcp.service" > "$USER_UNITS/jelly-mcp.service"
sed "s|/data/jelly|$root_escaped|g" "$ROOT/systemd/jelly-cloudflared.service" > "$USER_UNITS/jelly-cloudflared.service"
systemctl --user daemon-reload

started_at="$(date +%s)"
systemctl --user enable jelly-mcp.service >/dev/null
systemctl --user restart jelly-mcp.service

if [[ "$hosting_mode" == "cloudflare-fixed" ]]; then
  systemctl --user enable jelly-cloudflared.service >/dev/null
  systemctl --user restart jelly-cloudflared.service
else
  systemctl --user disable --now jelly-cloudflared.service >/dev/null 2>&1 || true
fi

printf '\n%b◇  Starting services%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
service_state="unknown"
for _ in $(seq 1 60); do
  service_state="$(systemctl --user is-active jelly-mcp.service 2>/dev/null || true)"
  [[ "$service_state" == "active" ]] && break
  [[ "$service_state" == "failed" ]] && break
  sleep 0.25
done

if [[ "$service_state" != "active" ]]; then
  printf '%b✕%b  jelly-mcp.service is %s.\n' "$C_RED$C_BOLD" "$C_RESET" "$service_state" >&2
  journalctl --user -u jelly-mcp.service --since "@$started_at" -n 40 --no-pager >&2 || true
  exit 1
fi

public_url=""
case "$hosting_mode" in
  quick-tunnel)
    for _ in $(seq 1 120); do
      public_url="$(journalctl --user -u jelly-mcp.service --since "@$started_at" --no-pager -o cat 2>/dev/null \
        | sed -n 's/^jelly Quick Tunnel: \(https:\/\/[^/[:space:]]*\)\/mcp$/\1/p' \
        | tail -1)"
      [[ -n "$public_url" ]] && break
      sleep 0.25
    done
    ;;
  cloudflare-fixed)
    public_url="${JELLY_PUBLIC_URL:-}"
    ;;
  nip-io)
    for _ in $(seq 1 120); do
      public_url="$(journalctl --user -u jelly-mcp.service --since "@$started_at" --no-pager -o cat 2>/dev/null \
        | sed -n 's/^jelly nip.io: \(https:\/\/[^/[:space:]]*\)\/mcp$/\1/p' \
        | tail -1)"
      [[ -n "$public_url" ]] && break
      sleep 0.25
    done
    ;;
esac

local_url="http://${JELLY_MCP_ADDR:-127.0.0.1:8787}"
health="unreachable"
if command -v curl >/dev/null 2>&1 && curl -fsS "$local_url/health" >/dev/null 2>&1; then
  health="healthy"
fi

if [[ "$consent_mode" == "paired" ]]; then
  printf '\n%b◇  Owner pairing%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
  if [[ -n "$public_url" ]]; then
    printf '  Pairing page    %b%s/pair%b\n' "$C_HONEY$C_BOLD" "$public_url" "$C_RESET"
  else
    printf '  Pairing page    %s/pair\n' "$local_url"
  fi
  printf '  Pairing secret  JELLY_BOOTSTRAP_SECRET in %s\n' "$ENV_FILE"
  paired=false
  spinner='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
  for i in $(seq 1 600); do
    if curl -fsS -H "Authorization: Bearer $bootstrap_secret" "$local_url/pair/status" 2>/dev/null \
      | grep -q '"paired":true'; then
      paired=true
      break
    fi
    frame="${spinner:$(((i - 1) % ${#spinner})):1}"
    printf '\r  %b%s%b  Waiting for owner pairing' "$C_HONEY$C_BOLD" "$frame" "$C_RESET"
    sleep 0.5
  done
  printf '\r\033[K'
  if [[ "$paired" != "true" ]]; then
    printf '%b✕%b  Pairing timed out; Jelly is still running. Open the pairing page and rerun the status command.\n' \
      "$C_RED$C_BOLD" "$C_RESET" >&2
    exit 3
  fi
  printf '%b✓%b  Owner browser paired.\n' "$C_GREEN$C_BOLD" "$C_RESET"
fi

printf '\n%b✓%b  Jelly is running.\n\n' "$C_GREEN$C_BOLD" "$C_RESET"
printf '%b◇  System summary%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
printf '  Service         %s\n' "$service_state"
printf '  Health          %s\n' "$health"
printf '  Hosting         %s\n' "$hosting_mode"
printf '  Local MCP       %s/mcp\n' "$local_url"
if [[ -n "$public_url" ]]; then
  printf '  Public MCP      %b%s/mcp%b\n' "$C_HONEY$C_BOLD" "$public_url" "$C_RESET"
  printf '  OAuth origin    %s\n' "$public_url"
  if [[ "$consent_mode" == "paired" ]]; then
    printf '  Pairing page    %s/pair\n' "$public_url"
  fi
elif [[ "$hosting_mode" != "local" ]]; then
  printf '  Public MCP      %bnot resolved yet%b\n' "$C_DIM" "$C_RESET"
fi
printf '  OAuth consent   %s\n' "$consent_mode"
printf '  ChatGPT DCR     %s\n' "${JELLY_OAUTH_PUBLIC_CHATGPT_DCR:-false}"
printf '  Config          %s %b(mode 0600)%b\n' "$ENV_FILE" "$C_DIM" "$C_RESET"
printf '  Runtime         /data/jelly-runtime\n'
printf '\n%b◇  Operations%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
printf '  Status          scripts/status-mcp-services.sh\n'
printf '  Logs            journalctl --user -u jelly-mcp.service -f\n'
printf '  Restart         systemctl --user restart jelly-mcp.service\n'
printf '  Stop            systemctl --user stop jelly-mcp.service\n'
