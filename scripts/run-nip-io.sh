#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/config.sh
source "$ROOT/scripts/config.sh"
BUILD_DIR="$CONFIG_BUILD_ROOT"
ADDR="${JELLY_MCP_ADDR:-127.0.0.1:8787}"
NETWORK_MODE="${JELLY_NIP_IO_NETWORK:-manual}"
HTTP_PORT="${JELLY_NIP_IO_HTTP_PORT:-8080}"
HTTPS_PORT="${JELLY_NIP_IO_HTTPS_PORT:-8443}"
MCP_BIN="${JELLY_MCP_BIN:-$BUILD_DIR/release/jelly-mcp}"
VALIDATE_IP_BIN="${JELLY_VALIDATE_PUBLIC_IP_BIN:-$BUILD_DIR/release/jelly-validate-public-ip}"
RUNTIME="$CONFIG_RUNTIME_ROOT/nip-io"
CADDYFILE="$RUNTIME/Caddyfile"

resolve_caddy() {
  if [[ -n "${JELLY_CADDY_BIN:-}" ]]; then
    [[ -x "$JELLY_CADDY_BIN" ]] || { echo "JELLY_CADDY_BIN is not executable" >&2; return 1; }
    printf '%s\n' "$JELLY_CADDY_BIN"
  elif command -v caddy >/dev/null 2>&1; then
    command -v caddy
  elif [[ -x "$HOME/.config/pilink/bin/caddy" ]]; then
    printf '%s\n' "$HOME/.config/pilink/bin/caddy"
  else
    echo "Caddy not found; install caddy or set JELLY_CADDY_BIN" >&2
    return 1
  fi
}

public_ipv4() {
  if [[ -n "${JELLY_PUBLIC_IPV4:-}" ]]; then
    printf '%s\n' "$JELLY_PUBLIC_IPV4"
    return
  fi
  local ip=""
  ip="$(curl -fsS --max-time 8 https://api.ipify.org 2>/dev/null || true)"
  [[ -n "$ip" ]] || ip="$(curl -fsS --max-time 8 https://checkip.amazonaws.com 2>/dev/null | tr -d '[:space:]' || true)"
  [[ -n "$ip" ]] || { echo "could not discover public IPv4; set JELLY_PUBLIC_IPV4" >&2; return 1; }
  printf '%s\n' "$ip"
}

validate_public_ipv4() {
  [[ -x "$VALIDATE_IP_BIN" ]] || {
    echo "public IPv4 validator not found: $VALIDATE_IP_BIN" >&2
    return 1
  }
  "$VALIDATE_IP_BIN" "$1"
}

local_ipv4() {
  ip -4 route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src") {print $(i+1); exit}}'
}

mapping_backend=""
release_mappings() {
  case "$mapping_backend" in
    upnpc)
      upnpc -d 80 TCP >/dev/null 2>&1 || true
      upnpc -d 443 TCP >/dev/null 2>&1 || true
      ;;
    natpmpc)
      natpmpc -a 0 80 tcp 0 >/dev/null 2>&1 || true
      natpmpc -a 0 443 tcp 0 >/dev/null 2>&1 || true
      ;;
  esac
}

open_mappings() {
  local lan_ip="$1"
  if command -v upnpc >/dev/null 2>&1; then
    # Fail closed: never overwrite router mappings that may belong to another application.
    local mappings
    mappings="$(upnpc -l)" || {
      echo "cannot inspect existing router mappings; refusing automatic changes" >&2
      return 1
    }
    if grep -Eq '(^|[^0-9])(80|443)->' <<<"$mappings"; then
      echo "existing router port mapping detected; use manual mode" >&2
      return 1
    fi
    upnpc -a "$lan_ip" "$HTTP_PORT" 80 TCP >/dev/null
    if ! upnpc -a "$lan_ip" "$HTTPS_PORT" 443 TCP >/dev/null; then
      upnpc -d 80 TCP >/dev/null 2>&1 || true
      return 1
    fi
    mapping_backend="upnpc"
    return 0
  fi
  if command -v natpmpc >/dev/null 2>&1; then
    echo "automatic NAT-PMP mapping ownership cannot be verified; use manual mode" >&2
    return 1
  fi
  echo "automatic nip.io networking requires upnpc or natpmpc" >&2
  return 1
}

mcp_pid=""
caddy_pid=""
cleanup() {
  if [[ -n "$caddy_pid" ]]; then
    kill "$caddy_pid" >/dev/null 2>&1 || true
    wait "$caddy_pid" >/dev/null 2>&1 || true
  fi
  if [[ -n "$mcp_pid" ]]; then
    kill "$mcp_pid" >/dev/null 2>&1 || true
    wait "$mcp_pid" >/dev/null 2>&1 || true
  fi
  release_mappings
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

case "$NETWORK_MODE" in
  auto|manual) ;;
  *) echo "JELLY_NIP_IO_NETWORK must be auto or manual" >&2; exit 2 ;;
esac

caddy="$(resolve_caddy)"
ip="$(public_ipv4)"
validate_public_ipv4 "$ip" || { echo "'$ip' is not a reachable public IPv4 address; nip.io cannot work behind private/CGNAT addressing" >&2; exit 2; }
hostname="${JELLY_NIP_IO_HOSTNAME:-jelly-${ip//./-}.nip.io}"
[[ "$hostname" =~ ^[a-z0-9-]+([.][a-z0-9-]+)+[.]nip[.]io$ || "$hostname" =~ ^[a-z0-9-]+[.]nip[.]io$ ]] || {
  echo "JELLY_NIP_IO_HOSTNAME must be a valid .nip.io hostname" >&2
  exit 2
}

if [[ "$NETWORK_MODE" == "auto" ]]; then
  lan_ip="$(local_ipv4)"
  [[ -n "$lan_ip" ]] || { echo "could not determine LAN IPv4 address" >&2; exit 2; }
  if ! open_mappings "$lan_ip"; then
    echo "automatic router mapping failed; use JELLY_NIP_IO_NETWORK=manual after forwarding public TCP 80->$HTTP_PORT and 443->$HTTPS_PORT" >&2
    exit 2
  fi
else
  cat >&2 <<MSG
nip.io manual network mode assumes these router forwards already exist:
  public TCP 80  -> this machine TCP $HTTP_PORT
  public TCP 443 -> this machine TCP $HTTPS_PORT
MSG
fi

mkdir -p "$RUNTIME"
chmod 700 "$RUNTIME"
cat > "$CADDYFILE" <<EOF_CADDY
{
  admin off
  persist_config off
  http_port $HTTP_PORT
  https_port $HTTPS_PORT
}

https://$hostname {
  reverse_proxy $ADDR
}
EOF_CADDY
chmod 600 "$CADDYFILE"

export JELLY_PUBLIC_URL="https://$hostname"
export JELLY_OAUTH_PUBLIC_CHATGPT_DCR="${JELLY_OAUTH_PUBLIC_CHATGPT_DCR:-true}"
export XDG_DATA_HOME="$RUNTIME/caddy-data"


"$MCP_BIN" &
mcp_pid=$!
"$caddy" run --config "$CADDYFILE" --adapter caddyfile &
caddy_pid=$!

printf 'jelly nip.io: https://%s/mcp\n' "$hostname"
printf 'Caddy local ports: HTTP %s / HTTPS %s\n' "$HTTP_PORT" "$HTTPS_PORT"

while true; do
  if ! kill -0 "$mcp_pid" >/dev/null 2>&1; then
    wait "$mcp_pid" || true
    echo "jelly MCP stopped; stopping Caddy" >&2
    exit 1
  fi
  if ! kill -0 "$caddy_pid" >/dev/null 2>&1; then
    wait "$caddy_pid" || true
    echo "Caddy stopped; stopping jelly MCP" >&2
    exit 1
  fi
  sleep 1
done
