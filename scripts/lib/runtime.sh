#!/usr/bin/env bash
# Sourceable functions only: no builds, traps, service actions, or .env loading.
# Intended for trusted repository scripts; do not source user-supplied paths.
# Never trust a caller-provided DEV_ROOT as a path to executable Bash config.
DEV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=../../config/dev.config.sh
source "$DEV_ROOT/config/dev.config.sh"
# shellcheck source=ui.sh
source "$DEV_ROOT/scripts/lib/ui.sh"

dev::error() { printf 'Jelly: %s\n' "$*" >&2; }
dev::require() {
  local tool
  for tool in "$@"; do
    command -v "$tool" >/dev/null 2>&1 || { dev::error "missing $tool (see docs/getting-started/REQUIREMENTS.md)"; return 2; }
  done
}

dev::doctor() {
  local failures=0 utility
  dev::ui_init
  dev::section 'Prerequisite check'
  for utility in git rustc cargo systemctl systemd-run bash base64 cp mv chmod mktemp grep sed awk date; do
    if command -v "$utility" >/dev/null 2>&1; then
      dev::check_ok "$utility"
    else
      dev::check_missing "$utility"
      failures=$((failures + 1))
    fi
  done
  if [[ -x /usr/bin/chromium ]]; then
    dev::check_ok 'Chromium (/usr/bin/chromium)'
  else
    dev::check_missing 'Chromium (/usr/bin/chromium)'
    failures=$((failures + 1))
  fi
  if command -v systemctl >/dev/null 2>&1 && systemctl --user is-system-running >/dev/null 2>&1; then
    dev::check_ok 'systemd user manager'
  else
    dev::check_missing 'systemd user manager'
    failures=$((failures + 1))
  fi
  if (( failures )); then
    dev::error "$failures prerequisite(s) missing or unavailable"
    return 2
  fi
  dev::success 'Core prerequisites are satisfied.'
}

dev::confirm() {
  local answer
  [[ -t 0 ]] || { dev::error 'explicit --yes required in noninteractive mode'; return 2; }
  read -r -p "$1 [y/N]: " answer || return 2
  [[ "$answer" == y || "$answer" == Y || "$answer" == yes || "$answer" == YES ]]
}
