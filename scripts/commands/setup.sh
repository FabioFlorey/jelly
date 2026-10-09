#!/usr/bin/env bash
set -euo pipefail

DEV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ROOT="$DEV_ROOT"
# shellcheck source=../lib/runtime.sh
source "$ROOT/scripts/lib/runtime.sh"
MODE=run
case "${1:-}" in
  '') ;;
  --dry-run) MODE=dry-run; shift ;;
  *) dev::error 'usage: ./scripts/dev.sh setup [--dry-run]'; exit 2 ;;
esac
(($# == 0)) || { dev::error 'unexpected setup arguments'; exit 2; }
# Check prerequisites BEFORE loading technical config or private .env data.
dev::doctor
# shellcheck source=../config.sh
source "$ROOT/scripts/lib/config.sh"
ENV_FILE="$ROOT/.env"
ENV_EXAMPLE="$ROOT/.env.example"
LOGO_FILE="$ROOT/$DEV_BRAND_LOGO"
LOGO_WIDTH="$DEV_UI_MAX_WIDTH"
dev::ui_init

declare -A CFG=()

banner() {
  local -a logo=()
  local line logo_width=0 tagline tagline_width pad=0 mode_label=""

  if [[ -f "$LOGO_FILE" ]]; then
    mapfile -t logo < "$LOGO_FILE"
  else
    logo=("jelly")
  fi

  for line in "${logo[@]}"; do
    (( ${#line} > logo_width )) && logo_width=${#line}
    if (( ${#line} > LOGO_WIDTH )); then
      warn "Logo line exceeds ${LOGO_WIDTH} characters: $LOGO_FILE"
      break
    fi
  done

  tagline="$I_BRAND  browser instrumentation for agents  ·  interactive MCP configuration"
  tagline_width=${#tagline}
  (( logo_width > tagline_width )) && pad=$(( (logo_width - tagline_width) / 2 ))

  printf '\n%b' "$C_HONEY$C_BOLD"
  for line in "${logo[@]}"; do
    printf '%s\n' "$line"
  done
  printf '%b\n' "$C_RESET"
  printf '%*s%b%s%b  browser instrumentation for agents  %b·%b  interactive MCP configuration\n' \
    "$pad" "" "$C_HONEY$C_BOLD" "$I_BRAND" "$C_RESET" "$C_HONEY" "$C_RESET"

  case "$MODE" in
    dry-run) mode_label="dry run · no changes will be written" ;;
  esac
  if [[ -n "$mode_label" ]]; then
    pad=0
    (( logo_width > ${#mode_label} )) && pad=$(( (logo_width - ${#mode_label}) / 2 ))
    printf '%*s%b%s%b\n' "$pad" "" "$C_DIM" "$mode_label" "$C_RESET"
  fi
  printf '\n'
}

# Setup-specific vocabulary delegates formatting to the shared UI helpers.
prompt_label() { printf '%b%s%b  %s' "$C_HONEY$C_BOLD" "$I_INFO" "$C_RESET" "$1"; }
section() { printf '%b%s  %s%b\n' "$C_HONEY$C_BOLD" "$1" "$2" "$C_RESET"; }
note() { printf '%b%s  %s%b\n' "$C_HONEY" "$I_INFO" "$1" "$C_RESET"; }
explain() { printf '   %b%s%b\n' "$C_DIM" "$1" "$C_RESET"; }
warn() { dev::warning "$1"; }
CHECK_FAILURES=0
check_ok() { dev::check_ok "$1"; }
check_missing() { dev::check_missing "$1"; CHECK_FAILURES=$((CHECK_FAILURES + 1)); }

check_command() {
  local command="$1" label="$2"
  if command -v "$command" >/dev/null 2>&1; then
    check_ok "$label ($(command -v "$command"))"
    return 0
  fi
  check_missing "$label"
  return 1
}

resolve_cloudflared_for_check() {
  if [[ -n "${CFG[JELLY_CLOUDFLARED_BIN]:-}" && -x "${CFG[JELLY_CLOUDFLARED_BIN]}" ]]; then
    printf '%s\n' "${CFG[JELLY_CLOUDFLARED_BIN]}"
  elif command -v cloudflared >/dev/null 2>&1; then
    command -v cloudflared
  elif [[ -x "$HOME/.config/pilink/bin/cloudflared" ]]; then
    printf '%s\n' "$HOME/.config/pilink/bin/cloudflared"
  else
    return 1
  fi
}

resolve_caddy_for_check() {
  if [[ -n "${CFG[JELLY_CADDY_BIN]:-}" && -x "${CFG[JELLY_CADDY_BIN]}" ]]; then
    printf '%s\n' "${CFG[JELLY_CADDY_BIN]}"
  elif command -v caddy >/dev/null 2>&1; then
    command -v caddy
  elif [[ -x "$HOME/.config/pilink/bin/caddy" ]]; then
    printf '%s\n' "$HOME/.config/pilink/bin/caddy"
  else
    return 1
  fi
}

check_selected_requirements() {
  local before="$CHECK_FAILURES" helper=""
  printf '\n'
  section "$I_NET" "Selected feature check"

  case "${CFG[JELLY_HOSTING_MODE]}" in
    quick-tunnel|cloudflare-fixed)
      if helper="$(resolve_cloudflared_for_check)"; then
        check_ok "cloudflared ($helper)"
      else
        check_missing "cloudflared (required for ${CFG[JELLY_HOSTING_MODE]})"
      fi
      ;;
    nip-io)
      check_command curl "curl (public IPv4 discovery)" || true
      check_command ip "iproute2/ip (LAN route discovery)" || true
      if helper="$(resolve_caddy_for_check)"; then
        check_ok "Caddy ($helper)"
      else
        check_missing "Caddy (required for nip-io)"
      fi
      if [[ "${CFG[JELLY_NIP_IO_NETWORK]}" == "auto" ]]; then
        if command -v upnpc >/dev/null 2>&1; then
          check_ok "UPnP helper ($(command -v upnpc))"
        elif command -v natpmpc >/dev/null 2>&1; then
          check_ok "NAT-PMP helper ($(command -v natpmpc))"
        else
          check_missing "upnpc or natpmpc (required for automatic nip.io router mappings)"
        fi
      fi
      ;;
  esac

  if [[ "${CFG[JELLY_OAUTH_CONSENT_MODE]}" == "paired" ]]; then
    check_command curl "curl (owner pairing status)" || true
  fi

  if [[ "${CFG[JELLY_CONFIGURE_TELEGRAM]:-false}" == "true" ]]; then
    check_command curl "curl (Telegram HITL)" || true
  fi

  if (( CHECK_FAILURES > before )); then
    printf '\n%b%s  Selected configuration has missing dependencies. Install them or choose a different mode.%b\n' "$C_RED$C_BOLD" "$I_FAIL" "$C_RESET" >&2
    exit 2
  fi
  printf '\n'
}

load_env() {
  [[ -f "$ENV_FILE" ]] || return 0
  local rows key encoded value
  if [[ "$MODE" == dry-run ]]; then
    local prebuilt="$CONFIG_BUILD_ROOT/debug/jelly-maint"
    if [[ ! -x "$prebuilt" ]]; then
      note 'Preview mode: existing .env is not read until jelly-maint is already built.'
      return 0
    fi
    rows="$("$prebuilt" env read "$ENV_FILE")" || return 2
  else
    rows="$(jelly_maint env read "$ENV_FILE")" || return 2
  fi
  while IFS=$'\t' read -r key encoded; do
    [[ -n "$key" ]] || continue
    value="$(printf '%s' "$encoded" | base64 --decode)" || return 2
    CFG["$key"]="$value"
  done <<< "$rows"
}

ask() {
  local key="$1" prompt="$2" default="${3:-}"
  local current="${CFG[$key]:-$default}" input="" label=""
  label="$(prompt_label "$prompt")"
  if [[ -n "$current" ]]; then
    read -r -p "$label ${C_DIM}[$current]${C_RESET}: " input
    CFG["$key"]="${input:-$current}"
  else
    read -r -p "$label: " input
    CFG["$key"]="$input"
  fi
}

ask_secret() {
  local key="$1" prompt="$2" generate="${3:-false}" min_len="${4:-0}"
  local current="${CFG[$key]:-}" input="" label=""
  label="$(prompt_label "$prompt")"
  if [[ -n "$current" ]]; then
    read -r -s -p "$label ${C_DIM}[Enter keeps existing]${C_RESET}: " input
    printf '\n'
    [[ -n "$input" ]] && CFG["$key"]="$input"
  elif [[ "$generate" == "true" ]]; then
    read -r -s -p "$label ${C_DIM}[Enter generates]${C_RESET}: " input
    printf '\n'
    if [[ -n "$input" ]]; then
      CFG["$key"]="$input"
    else
      CFG["$key"]="$(generate_secret)"
      printf '%b%s%b  Generated %s locally.\n' "$C_HONEY" "$I_LOCK" "$C_RESET" "$key"
    fi
  else
    read -r -s -p "$label: " input
    printf '\n'
    CFG["$key"]="$input"
  fi
  if (( min_len > 0 )) && (( ${#CFG[$key]} < min_len )); then
    echo "$key must be at least $min_len bytes" >&2
    exit 2
  fi
}

ask_choice() {
  local key="$1" prompt="$2" allowed="$3" default="$4"
  local current="${CFG[$key]:-$default}" input="" label=""
  label="$(prompt_label "$prompt")"
  while true; do
    read -r -p "$label ${C_DIM}[$current]${C_RESET}: " input
    input="${input:-$current}"
    for choice in $allowed; do
      if [[ "$input" == "$choice" ]]; then
        CFG["$key"]="$input"
        return
      fi
    done
    echo "Choose one of: $allowed" >&2
  done
}

ask_bool() {
  local key="$1" prompt="$2" default="${3:-false}"
  local current="${CFG[$key]:-$default}" input="" label=""
  local hint="y/N"
  [[ "$current" == "true" ]] && hint="Y/n"
  label="$(prompt_label "$prompt")"
  while true; do
    read -r -p "$label ${C_DIM}[$hint]${C_RESET}: " input
    input="${input,,}"
    if [[ -z "$input" ]]; then
      CFG["$key"]="$current"
      return
    fi
    case "$input" in
      y|yes) CFG["$key"]="true"; return ;;
      n|no) CFG["$key"]="false"; return ;;
      *) echo "Answer y or n." >&2 ;;
    esac
  done
}

generate_secret() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32
  else
    od -An -N32 -tx1 /dev/urandom | tr -d ' \n'
  fi
}

print_redacted_summary() {
  printf '\n'
  section "$I_SECTION" "Configuration summary"
  printf '  MCP token: configured (%s bytes)\n' "${#CFG[JELLY_MCP_TOKEN]}"
  printf '  Bootstrap secret: configured (%s bytes)\n' "${#CFG[JELLY_BOOTSTRAP_SECRET]}"
  printf '  OAuth consent: %s\n' "${CFG[JELLY_OAUTH_CONSENT_MODE]}"
  if [[ "${CFG[JELLY_OAUTH_CONSENT_MODE]}" == "browser" ]]; then
    printf '  OAuth password: configured (%s bytes)\n' "${#CFG[JELLY_OAUTH_PASSWORD]}"
  else
    printf '  OAuth password: not required\n'
  fi
  printf '  Public ChatGPT DCR: %s\n' "${CFG[JELLY_OAUTH_PUBLIC_CHATGPT_DCR]}"
  printf '  Hosting: %s\n' "${CFG[JELLY_HOSTING_MODE]}"
  printf '  MCP listen address: %s\n' "${CFG[JELLY_MCP_ADDR]}"
  case "${CFG[JELLY_HOSTING_MODE]}" in
    cloudflare-fixed)
      printf '  Public URL: %s\n' "${CFG[JELLY_PUBLIC_URL]}"
      printf '  Cloudflare tunnel token: configured\n'
      ;;
    quick-tunnel)
      printf '  Public URL: generated at runtime\n'
      ;;
    nip-io)
      printf '  nip.io network: %s\n' "${CFG[JELLY_NIP_IO_NETWORK]}"
      printf '  nip.io public IPv4: %s\n' "${CFG[JELLY_PUBLIC_IPV4]:-auto-detect}"
      printf '  nip.io hostname: %s\n' "${CFG[JELLY_NIP_IO_HOSTNAME]:-derived automatically}"
      ;;
    local)
      printf '  Public URL: none\n'
      ;;
  esac
  if [[ -n "${CFG[JELLY_TELEGRAM_BOT_TOKEN]:-}" && -n "${CFG[JELLY_TELEGRAM_CHAT_ID]:-}" ]]; then
    printf '  Telegram HITL: configured\n'
  else
    printf '  Telegram HITL: not configured\n'
  fi
}

write_env() {
  # Serialize as data, not shell source. The writer atomically replaces .env.
  local key
  for key in "${!CFG[@]}"; do
    [[ "$key" == JELLY_* ]] || continue
    printf '%s\t%s\n' "$key" "$(printf '%s' "${CFG[$key]}" | base64 | tr -d '\n')"
  done | jelly_maint env write-rows "$ENV_FILE"
}

[[ -t 0 ]] || echo "Warning: setup is intended for an interactive terminal." >&2
[[ -f "$ENV_EXAMPLE" ]] || { echo "missing $ENV_EXAMPLE" >&2; exit 2; }

load_env

banner
if [[ -f "$ENV_FILE" ]]; then
  note "Existing .env detected. Press Enter to keep existing values."
  echo
fi

explain "Used by direct MCP clients. Press Enter and Jelly will generate a secure token for you."
ask_secret JELLY_MCP_TOKEN "MCP bearer token" true 32

explain "Protects owner setup such as browser pairing. Press Enter to generate it automatically."
ask_secret JELLY_BOOTSTRAP_SECRET "OAuth bootstrap secret" true 32

explain "Choose paired for an owner browser approval flow, or browser to approve each OAuth request with a password."
ask_choice JELLY_OAUTH_CONSENT_MODE \
  "OAuth consent mode (browser asks for a password; paired pairs an owner browser)" \
  "browser paired" browser

if [[ "${CFG[JELLY_OAUTH_CONSENT_MODE]}" == "browser" ]]; then
  explain "Password requested on Jelly's authorization page. Press Enter to generate one."
  ask_secret JELLY_OAUTH_PASSWORD "OAuth approval password" true 16
else
  CFG[JELLY_OAUTH_PASSWORD]="${CFG[JELLY_OAUTH_PASSWORD]:-}"
fi

explain "Enable this for ChatGPT so it can register itself as an OAuth client automatically."
ask_bool JELLY_OAUTH_PUBLIC_CHATGPT_DCR \
  "Allow public Dynamic Client Registration for ChatGPT callbacks" true

explain "Choose how Jelly becomes reachable. quick-tunnel is the easiest temporary public URL."
ask_choice JELLY_HOSTING_MODE \
  "Hosting mode" \
  "local quick-tunnel nip-io cloudflare-fixed" \
  quick-tunnel

explain "Local address Jelly listens on. The default is correct unless that port is already in use."
ask JELLY_MCP_ADDR "Local MCP listen address" "127.0.0.1:8787"

case "${CFG[JELLY_HOSTING_MODE]}" in
  local)
    CFG[JELLY_PUBLIC_URL]=""
    ;;

  quick-tunnel)
    CFG[JELLY_PUBLIC_URL]=""
    explain "Leave this blank unless you want Jelly to use a specific cloudflared executable."
    ask JELLY_CLOUDFLARED_BIN "cloudflared executable override (blank = auto-detect)" ""
    ;;

  cloudflare-fixed)
    explain "Enter the permanent HTTPS origin assigned to your named Cloudflare tunnel. Do not include /mcp."
    while true; do
      ask JELLY_PUBLIC_URL "Stable public HTTPS origin, e.g. https://jelly.example.com" "${CFG[JELLY_PUBLIC_URL]:-}"
      if [[ "${CFG[JELLY_PUBLIC_URL]}" =~ ^https://[^/]+$ ]]; then break; fi
      echo "Enter a bare HTTPS origin without a path." >&2
    done
    explain "Paste the token Cloudflare gives you for this named tunnel."
    ask_secret JELLY_CLOUDFLARE_TUNNEL_TOKEN "Cloudflare named-tunnel token" false 1
    explain "Leave this blank unless cloudflared is installed somewhere unusual."
    ask JELLY_CLOUDFLARED_BIN "cloudflared executable override (blank = auto-detect)" ""
    ;;

  nip-io)
    CFG[JELLY_PUBLIC_URL]=""
    explain "Choose manual if you will forward router ports yourself; auto tries UPnP/NAT-PMP for you."
    ask_choice JELLY_NIP_IO_NETWORK "nip.io router setup" "manual auto" manual
    explain "Leave blank to detect your public IPv4 automatically."
    ask JELLY_PUBLIC_IPV4 "Public IPv4 override (blank = auto-detect)" ""
    explain "Leave blank to derive the nip.io hostname from your public IP."
    ask JELLY_NIP_IO_HOSTNAME "nip.io hostname override (blank = derive from public IPv4)" ""
    explain "Leave blank unless you want to use a specific Caddy executable."
    ask JELLY_CADDY_BIN "Caddy executable override (blank = auto-detect)" ""
    explain "Local ports used by Caddy. Keep these defaults unless they conflict with another service."
    ask JELLY_NIP_IO_HTTP_PORT "Local Caddy HTTP port" "8080"
    ask JELLY_NIP_IO_HTTPS_PORT "Local Caddy HTTPS port" "8443"
    if [[ "${CFG[JELLY_NIP_IO_NETWORK]}" == "manual" ]]; then
      printf '\n'
      section "$I_NET" "Router forwarding required"
      printf '  public TCP 80  -> this machine TCP %s\n' "${CFG[JELLY_NIP_IO_HTTP_PORT]}"
      printf '  public TCP 443 -> this machine TCP %s\n\n' "${CFG[JELLY_NIP_IO_HTTPS_PORT]}"
    fi
    ;;
esac

explain "Optional: connect Telegram so Jelly can ask you for help when a workflow needs a human."
ask_bool JELLY_CONFIGURE_TELEGRAM "Configure Telegram HITL now" \
  "$([[ -n "${CFG[JELLY_TELEGRAM_BOT_TOKEN]:-}" && -n "${CFG[JELLY_TELEGRAM_CHAT_ID]:-}" ]] && echo true || echo false)"
if [[ "${CFG[JELLY_CONFIGURE_TELEGRAM]}" == "true" ]]; then
  explain "Paste the token issued by BotFather for the bot Jelly should send through."
  ask_secret JELLY_TELEGRAM_BOT_TOKEN "Telegram bot token" false 1
  explain "Enter the chat ID that should receive Jelly's human-in-the-loop messages."
  ask_secret JELLY_TELEGRAM_CHAT_ID "Telegram chat ID" false 1
fi

check_selected_requirements
unset 'CFG[JELLY_CONFIGURE_TELEGRAM]'

if [[ "$MODE" == "dry-run" ]]; then
  print_redacted_summary
  printf '\n%b%s%b  Validation complete. No changes made.\n' "$C_GREEN$C_BOLD" "$I_OK" "$C_RESET"
  exit 0
fi

if [[ -f "$ENV_FILE" ]]; then
  backup="$ENV_FILE.backup.$(date +%Y%m%d%H%M%S%N)"
  cp -p "$ENV_FILE" "$backup"
  printf 'Backup: %s\n' "$backup"
fi

write_env
printf '\n%b%s%b  Configuration written to %s %b(mode 0600)%b.\n' "$C_GREEN$C_BOLD" "$I_OK" "$C_RESET" "$ENV_FILE" "$C_DIM" "$C_RESET"
printf 'Hosting: %s\n' "${CFG[JELLY_HOSTING_MODE]}"
printf 'OAuth consent: %s\n' "${CFG[JELLY_OAUTH_CONSENT_MODE]}"
printf 'Public ChatGPT DCR: %s\n' "${CFG[JELLY_OAUTH_PUBLIC_CHATGPT_DCR]}"

explain "Choose yes to apply this configuration and start Jelly now. Existing release binaries are reused when current."
ask_bool JELLY_INSTALL_NOW "Build and install/start the user services now" true
if [[ "${CFG[JELLY_INSTALL_NOW]}" == "true" ]]; then
  exec "$ROOT/scripts/commands/install.sh"
fi

printf '\n%b%s  Configuration complete%b\n' "$C_GREEN$C_BOLD" "$I_OK" "$C_RESET"
printf '  Start later with: %b./scripts/dev.sh install%b\n' "$C_HONEY" "$C_RESET"
