#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/config.sh
source "$ROOT/scripts/config.sh"
USER_UNITS="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
ENV_FILE="$ROOT/.env"

if [[ ! -f "$ENV_FILE" ]]; then
  echo "missing $ENV_FILE; copy .env.example and configure MCP secrets first" >&2
  exit 2
fi
chmod 600 "$ENV_FILE"

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
BUILD_DIR="$CONFIG_BUILD_ROOT"
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
  "$ROOT/assets/jelly.css" \
  "$ROOT/assets/fonts" \
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
[[ "$ROOT" != *'|'* && "$ROOT" != *$'\n'* ]] || {
  echo "repository path contains unsupported characters for service templates" >&2
  exit 2
}
root_escaped="${ROOT//\\/\\\\}"
root_escaped="${root_escaped//&/\\&}"
mcp_unit_tmp="$(mktemp "$USER_UNITS/.jelly-mcp.XXXXXX")"
tunnel_unit_tmp="$(mktemp "$USER_UNITS/.jelly-cloudflared.XXXXXX")"
cleanup_unit_temps() { rm -f -- "$mcp_unit_tmp" "$tunnel_unit_tmp"; }
trap cleanup_unit_temps EXIT
sed "s|/data/jelly|$root_escaped|g" "$ROOT/systemd/jelly-mcp.service" > "$mcp_unit_tmp"
sed "s|/data/jelly|$root_escaped|g" "$ROOT/systemd/jelly-cloudflared.service" > "$tunnel_unit_tmp"
chmod 600 "$mcp_unit_tmp" "$tunnel_unit_tmp"
mv -f -- "$mcp_unit_tmp" "$USER_UNITS/jelly-mcp.service"
mv -f -- "$tunnel_unit_tmp" "$USER_UNITS/jelly-cloudflared.service"
trap - EXIT
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
admin_url="http://${JELLY_MCP_ADMIN_ADDR:-127.0.0.1:8788}"
health="unreachable"
chatgpt_authorized=false
if command -v curl >/dev/null 2>&1 && curl -fsS "$local_url/health" >/dev/null 2>&1; then
  health="healthy"
  oauth_status="$(curl -fsS -H "Authorization: Bearer $bootstrap_secret" "$local_url/status.json" 2>/dev/null || true)"
  if [[ "$oauth_status" == *'"chatgpt_authorized":true'* ]]; then
    chatgpt_authorized=true
  fi
fi

if [[ "$consent_mode" == "paired" ]]; then
  local_approved=false

  if [[ "$chatgpt_authorized" == "true" ]]; then
    local_approved=true
    printf '\n%b✓%b  Existing ChatGPT OAuth authorization found; owner setup skipped.\n' \
      "$C_GREEN$C_BOLD" "$C_RESET"
  fi

  if [[ "$local_approved" != "true" && "${JELLY_OAUTH_PUBLIC_CHATGPT_DCR:-false}" == "true" && -t 0 && -t 1 ]]; then
    # Authorize setup from the installer; a bare GET /connect never creates it.
    setup_json="$(curl -fsS -X POST -H "Authorization: Bearer $bootstrap_secret" \
      "$admin_url/admin/oauth/setup/open" 2>/dev/null || true)"
    browser_connect_url="$(printf '%s' "$setup_json" | sed -n 's/.*"connect_url":"\([^"]*\)".*/\1/p')"
    if [[ -n "$public_url" ]]; then
      mcp_url="$public_url/mcp"
    else
      mcp_url="$local_url/mcp"
    fi

    if [[ -n "$browser_connect_url" ]]; then
      printf '\n%b◇  Jelly browser setup%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
      printf '  MCP URL         %b%s%b\n' "$C_HONEY$C_BOLD" "$mcp_url" "$C_RESET"
    printf '  Connect page    %b%s%b\n' "$C_HONEY$C_BOLD" "$browser_connect_url" "$C_RESET"

    opened=false
    if [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]] && command -v xdg-open >/dev/null 2>&1; then
      xdg-open "$browser_connect_url" >/dev/null 2>&1 &
      opened=true
    elif [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]] && command -v gio >/dev/null 2>&1; then
      gio open "$browser_connect_url" >/dev/null 2>&1 &
      opened=true
    elif command -v open >/dev/null 2>&1; then
      open "$browser_connect_url" >/dev/null 2>&1 &
      opened=true
    fi

    if [[ "$opened" == "true" ]]; then
      local_approved=true
      printf '  Browser         opened using an authorized short-lived setup link.\n'
    else
      printf '%b!%b  Could not open /connect automatically; using browser pairing fallback.\n' \
        "$C_HONEY$C_BOLD" "$C_RESET"
    fi
    else
      printf '%b!%b  Authorized setup link unavailable; using browser pairing fallback.\n' \
        "$C_HONEY$C_BOLD" "$C_RESET"
    fi
  fi

  if [[ "$local_approved" != "true" ]]; then
    printf '\n%b◇  Owner pairing%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
    pair_json="$(curl -fsS -X POST -H "Authorization: Bearer $bootstrap_secret" "$local_url/pair/code" 2>/dev/null || true)"
    pair_url="$(printf '%s' "$pair_json" | sed -n 's/.*"pair_url":"\([^"]*\)".*/\1/p')"
    if [[ -z "$pair_url" ]]; then
      if [[ -n "$public_url" ]]; then
        pair_url="$public_url/pair"
      else
        pair_url="$local_url/pair"
      fi
      printf '  Pairing page    %b%s%b\n' "$C_HONEY$C_BOLD" "$pair_url" "$C_RESET"
      printf '  Pairing secret  JELLY_BOOTSTRAP_SECRET in %s\n' "$ENV_FILE"
    else
      printf '  One-time link   %b%s%b\n' "$C_HONEY$C_BOLD" "$pair_url" "$C_RESET"
      opened=false
      if [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]] && command -v xdg-open >/dev/null 2>&1; then
        xdg-open "$pair_url" >/dev/null 2>&1 &
        opened=true
      elif [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]] && command -v gio >/dev/null 2>&1; then
        gio open "$pair_url" >/dev/null 2>&1 &
        opened=true
      elif command -v open >/dev/null 2>&1; then
        open "$pair_url" >/dev/null 2>&1 &
        opened=true
      fi
      if [[ "$opened" == "true" ]]; then
        printf '  Browser         opened automatically (link expires in 5 minutes)\n'
      else
        printf '  Browser         open the one-time link above (expires in 5 minutes)\n'
      fi
    fi
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
printf '  Environment     %s %b(mode 0600)%b\n' "$ENV_FILE" "$C_DIM" "$C_RESET"
printf '  Config          %s\n' "$CONFIG_FILE"
printf '  Runtime         %s\n' "$CONFIG_RUNTIME_ROOT"
printf '\n%b◇  Operations%b\n' "$C_HONEY$C_BOLD" "$C_RESET"
printf '  Status          scripts/status-mcp-services.sh\n'
printf '  Logs            journalctl --user -u jelly-mcp.service -f\n'
printf '  Restart         systemctl --user restart jelly-mcp.service\n'
printf '  Stop            systemctl --user stop jelly-mcp.service\n'