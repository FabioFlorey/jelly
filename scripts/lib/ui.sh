#!/usr/bin/env bash
# Shell-specific presentation helpers. Branding is configured in config/dev.config.sh.
dev::ui_init() {
  C_RESET='' C_BOLD='' C_DIM='' C_RED='' C_GREEN='' C_YELLOW='' C_HONEY=''
  if [[ -t 1 && -z "${NO_COLOR:-}" && "${TERM:-dumb}" != dumb ]]; then
    C_RESET=$'\033[0m'
    C_BOLD=$'\033[1m'
    C_DIM=$'\033[2m'
    C_RED=$'\033['"${DEV_COLOR_ERROR}"'m'
    C_GREEN=$'\033['"${DEV_COLOR_SUCCESS}"'m'
    C_YELLOW=$'\033['"${DEV_COLOR_WARNING}"'m'
    C_HONEY=$'\033[38;2;'"${DEV_COLOR_HONEY}"'m'
  fi
  if [[ "${DEV_UI_ICONS}" == true ]]; then
    I_BRAND='◆' I_OK='✓' I_FAIL='✕' I_INFO='›' I_SECTION='◇' I_LOCK='♦' I_NET='↗'
  else
    I_BRAND='*' I_OK='+' I_FAIL='x' I_INFO='>' I_SECTION='>' I_LOCK='>' I_NET='>'
  fi
}
dev::section() { printf '\n%b%s  %s%b\n' "$C_HONEY$C_BOLD" "$I_SECTION" "$1" "$C_RESET"; }
dev::success() { printf '%b%s%b  %s\n' "$C_GREEN$C_BOLD" "$I_OK" "$C_RESET" "$1"; }
dev::warning() { printf '%b%s%b  %s\n' "$C_YELLOW" "$I_INFO" "$C_RESET" "$1" >&2; }
dev::check_ok() { printf '  %b%s%b  %s\n' "$C_GREEN$C_BOLD" "$I_OK" "$C_RESET" "$1"; }
dev::check_missing() { printf '  %b%s%b  %s\n' "$C_RED$C_BOLD" "$I_FAIL" "$C_RESET" "$1"; }
