#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/config.sh
source "$ROOT/scripts/config.sh"
ENV_FILE="$ROOT/.env"

if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
  C_RESET=$'\033[0m'; C_BOLD=$'\033[1m'; C_DIM=$'\033[2m'
  C_GREEN=$'\033[32m'; C_RED=$'\033[31m'; C_HONEY=$'\033[38;2;255;193;7m'
else
  C_RESET=''; C_BOLD=''; C_DIM=''; C_GREEN=''; C_RED=''; C_HONEY=''
fi

state="$(systemctl --user is-active jelly-mcp.service 2>/dev/null || true)"
enabled="$(systemctl --user is-enabled jelly-mcp.service 2>/dev/null || true)"
hosting="${JELLY_HOSTING_MODE:-local}"
consent="${JELLY_OAUTH_CONSENT_MODE:-browser}"
local_url="http://${JELLY_MCP_ADDR:-127.0.0.1:8787}"
health="unreachable"
if command -v curl >/dev/null 2>&1 && curl -fsS "$local_url/health" >/dev/null 2>&1; then
  health="healthy"
fi

public_url="${JELLY_PUBLIC_URL:-}"
if [[ "$hosting" == "quick-tunnel" ]]; then
  public_url="$(journalctl --user -u jelly-mcp.service -n 300 --no-pager -o cat 2>/dev/null \
    | sed -n 's/^jelly Quick Tunnel: \(https:\/\/[^/[:space:]]*\)\/mcp$/\1/p' \
    | tail -1)"
elif [[ "$hosting" == "nip-io" ]]; then
  public_url="$(journalctl --user -u jelly-mcp.service -n 300 --no-pager -o cat 2>/dev/null \
    | sed -n 's/^jelly nip.io: \(https:\/\/[^/[:space:]]*\)\/mcp$/\1/p' \
    | tail -1)"
fi

printf '%b◇  Jelly system%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
printf '  Service         %s (%s)\n' "$state" "$enabled"
printf '  Health          %s\n' "$health"
printf '  Hosting         %s\n' "$hosting"
printf '  Local MCP       %s/mcp\n' "$local_url"
if [[ -n "$public_url" ]]; then
  printf '  Public MCP      %b%s/mcp%b\n' "$C_HONEY$C_BOLD" "$public_url" "$C_RESET"
  printf '  OAuth origin    %s\n' "$public_url"
  [[ "$consent" == "paired" ]] && printf '  Pairing page    %s/pair\n' "$public_url"
fi
printf '  OAuth consent   %s\n' "$consent"
printf '  ChatGPT DCR     %s\n' "${JELLY_OAUTH_PUBLIC_CHATGPT_DCR:-false}"
printf '  Environment     %s\n' "$ENV_FILE"
printf '  Config          %s\n' "$CONFIG_FILE"
printf '  Runtime         %s\n' "$CONFIG_RUNTIME_ROOT"
printf '\n%b◇  Operations%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
printf '  Logs            journalctl --user -u jelly-mcp.service -f\n'
printf '  Restart         systemctl --user restart jelly-mcp.service\n'
printf '  Stop            systemctl --user stop jelly-mcp.service\n'
