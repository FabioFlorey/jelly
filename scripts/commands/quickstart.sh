#!/usr/bin/env bash
# Real, opt-in Jelly quickstart. Browser setup, MCP credentials and service
# installation are separate confirmed operations; never use demo fixtures.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=../lib/runtime.sh
source "$ROOT/scripts/lib/runtime.sh"
dev::ui_init

usage() {
  cat <<'HELP'
Usage: ./scripts/dev.sh [quickstart]
Guided real setup: prerequisites → browser → MCP access → install/start → verify.

Requires an interactive terminal. Nothing is installed or changed without
review and confirmation. Run './scripts/dev.sh demo' for a simulation.
HELP
}

case "${1:-}" in
  '') ;;
  --help|-h) (($# == 1)) || exit 2; usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac
(($# <= 1)) || { usage >&2; exit 2; }
if [[ ! -t 0 || ! -t 1 ]]; then
  dev::error 'Quickstart needs an interactive terminal. Use demo for a preview or explicit commands for automation.'
  exit 2
fi

dev::brand full
dev::chapter '01/04' 'Prerequisites'
# A new user may choose remote/container Chromium, so do not insist on host
# Chromium until after browser selection. Still require the real user manager.
dev::doctor --skip-browser

dev::chapter '02/04' 'Browser configuration'
dev::note 'Choose where Jelly will run Chromium. Each change requires confirmation.'
dev::note 'Confirmed browser changes persist even if you cancel the later MCP setup.'
dev::option 1 'Native Chromium on this machine'
dev::option 2 'Docker / Podman Chromium (interactive configuration)'
dev::option 3 'Attach an existing CDP Chromium'
dev::option 4 'Keep previously configured browser'
dev::option 0 'Cancel quickstart'

choice=''
while true; do
  dev::prompt 'Browser option [0-4]: ' choice || exit 2
  case "$choice" in
    1)
      if [[ ! -x /usr/bin/chromium ]]; then
        dev::warning 'Host Chromium is not installed at /usr/bin/chromium; choose a container or an existing CDP browser.'
        continue
      fi
      dev::section 'REVIEW  /  Native Chromium'
      dev::item 'Browser backend' 'native'
      dev::item 'Configuration' "$ROOT/config/jelly.toml"
      if dev::ask_yes 'Use the native Chromium backend?'; then
        bash "$ROOT/scripts/commands/browser-container.sh" native
      else
        dev::note 'No browser settings changed.'
        continue
      fi
      break ;;
    2)
      # The real browser menu reviews/asks before modifying any backend or
      # building containers; returning from a menu is not evidence of success.
      bash "$ROOT/scripts/commands/browser-container.sh" menu
      dev::section 'BROWSER REVIEW'
      if grep -q '^backend = "remote-cdp"$' "$ROOT/config/jelly.toml"; then
        dev::item 'Current backend' 'remote-cdp'
      else
        dev::item 'Current backend' 'native'
        dev::note 'No remote browser backend is selected. The native backend requires host Chromium.'
      fi
      if dev::ask_yes 'Continue with the currently configured browser?'; then break; fi
      ;;
    3)
      address=''
      dev::prompt 'Existing CDP endpoint (HOST:PORT or URL): ' address || exit 2
      if [[ -z "$address" ]]; then
        dev::note 'No address entered.'
        continue
      fi
      dev::section 'REVIEW  /  External Chromium'
      dev::item 'CDP endpoint' "$address"
      dev::note 'Anyone who can reach an unauthenticated CDP endpoint may control that browser.'
      if dev::ask_yes 'Register this CDP browser?'; then
        if ! bash "$ROOT/scripts/commands/browser-container.sh" connect --address "$address"; then
          dev::error 'Browser registration failed. Correct the address before continuing.'
          exit 1
        fi
        break
      fi
      dev::note 'No external browser registered.' ;;
    4)
      backend=native
      grep -q '^backend = "remote-cdp"$' "$ROOT/config/jelly.toml" && backend=remote-cdp
      dev::item 'Existing browser backend' "$backend"
      if dev::ask_yes 'Reuse the current browser configuration?'; then break; fi ;;
    0)
      dev::note 'Quickstart cancelled.'
      exit 0 ;;
    *) dev::note 'Choose one of the displayed options.' ;;
  esac
done

dev::chapter '03/04' 'MCP identity, hosting and integrations'
# The wizard includes its own real dependency checks and a redacted review.
# In quickstart mode the wizard saves only, and aborts with a distinct status
# on rejection so the orchestrator never continues to service installation.
setup_status=0
bash "$ROOT/scripts/commands/setup.sh" --quickstart || setup_status=$?
if ((setup_status != 0)); then
  if ((setup_status == 3)); then
    dev::note 'Quickstart stopped at setup review. Service installation was not started.'
    exit 0
  fi
  dev::error 'MCP setup did not complete; no service installation was requested.'
  exit "$setup_status"
fi

dev::chapter '04/04' 'Install, start and verify'
dev::item 'Service action' 'Build binaries, install user services and start Jelly'
dev::item 'Next step' 'Verify local MCP health and readiness'
if ! dev::ask_yes 'Install and start Jelly using the saved configuration?'; then
  dev::note 'Settings saved. Start later using ./scripts/dev.sh install.'
  exit 0
fi

if ! bash "$ROOT/scripts/commands/install.sh"; then
  dev::error 'Service installation/startup did not complete. Inspect ./scripts/dev.sh status and logs.'
  exit 1
fi

# Install builds the maintenance parser. Load only validated .env values as
# data, never by sourcing the .env file or printing its secrets.
# shellcheck source=../lib/config.sh
source "$ROOT/scripts/lib/config.sh"
jelly_load_env --existing-bin
local_url="http://${JELLY_MCP_ADDR:-127.0.0.1:8787}"
if ! curl --fail --silent --show-error --max-time 5 "$local_url/health" >/dev/null; then
  dev::error "MCP health check failed at $local_url/health; inspect status and logs."
  exit 1
fi
if ! curl --fail --silent --show-error --max-time 5 "$local_url/ready" >/dev/null; then
  dev::error "MCP readiness check failed at $local_url/ready; inspect status and logs."
  exit 1
fi
dev::success 'Jelly MCP is running and its health/readiness endpoints are reachable.'
dev::note 'Browser readiness is separate; confirm its CDP connectivity or browser tools before production use.'
dev::note 'Inspect: ./scripts/dev.sh status'
