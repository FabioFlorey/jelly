#!/usr/bin/env bash
# Shell-specific presentation helpers. Branding is configured in config/dev.config.sh.
dev::ui_init() {
  C_RESET='' C_BOLD='' C_DIM='' C_RED='' C_GREEN='' C_YELLOW='' C_HONEY=''
  if [[ -t 1 && -z "${NO_COLOR+x}" && "${TERM:-dumb}" != dumb ]]; then
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

# One presentation vocabulary for setup, the landing screen and browser menus.
# Never animate or emit ANSI to redirected output or when NO_COLOR is set.
dev::columns() {
  local width="${COLUMNS:-80}"
  if [[ -t 1 ]] && command -v tput >/dev/null 2>&1; then
    width="$(tput cols 2>/dev/null || printf '%s' "$width")"
  fi
  [[ "$width" =~ ^[0-9]+$ ]] && ((width > 0)) || width=80
  printf '%s\n' "$width"
}

dev::brand() {
  local mode="${1:-compact}" width line logo_width=0
  width="$(dev::columns)"
  # Normal screens show the full 97-column artwork only in wide terminals.
  # The explicit 'hero' preview deliberately shows the original, even in
  # redirected output or narrow terminals, so users can inspect the asset.
  if { [[ "$mode" == hero ]] || { [[ "$mode" == full && -t 1 && "$width" -ge 100 ]]; }; } && [[ -s "$DEV_ROOT/$DEV_BRAND_LOGO" ]]; then
    local -a logo=()
    mapfile -t logo < "$DEV_ROOT/$DEV_BRAND_LOGO"
    for line in "${logo[@]}"; do
      if [[ "$mode" != hero ]] && ((${#line} > width)); then logo=(); break; fi
      ((${#line} > logo_width)) && logo_width=${#line}
    done
    if ((${#logo[@]})); then
      printf '\n%b' "$C_HONEY$C_BOLD"
      printf '%s\n' "${logo[@]}"
      printf '%b' "$C_RESET"
    fi
  fi
  if ((logo_width == 0)); then
    printf '\n  %b%s  JELLY%b\n' "$C_HONEY$C_BOLD" "$I_BRAND" "$C_RESET"
  fi
  local subtitle="Browser instrumentation for agents" pad=2
  if ((width < 44)); then subtitle="Browser agents"; fi
  if ((logo_width > ${#subtitle})); then
    pad=$(( (logo_width - ${#subtitle}) / 2 ))
  fi
  printf '%*s%b%s%b\n\n' "$pad" "" "$C_DIM" "$subtitle" "$C_RESET"
}

dev::chapter() {
  # Two-line chapter boundary; one blank line between heading and content.
  printf '\n\n%b━━  %s  /  %s  ━━%b\n\n' "$C_HONEY$C_BOLD" "$1" "$2" "$C_RESET"
}
dev::subchapter() { printf '\n%b%s%b\n\n' "$C_HONEY$C_BOLD" "$1" "$C_RESET"; }
dev::step() { dev::subchapter "STEP $1 · $2"; }
dev::item() { printf '  %b%-18s%b %s\n' "$C_DIM" "$1" "$C_RESET" "$2"; }
dev::option() { printf '  %b%2s%b  %s\n' "$C_HONEY$C_BOLD" "$1" "$C_RESET" "$2"; }
dev::note() { printf '  %b%s%b  %s\n' "$C_HONEY" "$I_INFO" "$C_RESET" "$1"; }
dev::prompt() { read -r -p "${C_HONEY}${I_INFO}${C_RESET}  $1" "$2"; }
dev::ask_yes() {
  local answer
  dev::prompt "$1 [y/N]: " answer || return 1
  [[ "$answer" == y || "$answer" == Y || "$answer" == yes || "$answer" == YES ]]
}
