#!/usr/bin/env bash
# Product UI simulation: only sample state, Bash builtins and shared styling.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "$ROOT/scripts/lib/runtime.sh"
dev::ui_init

SAMPLE_ENV=podman
SAMPLE_SERVICE=stopped
SAMPLE_CLIENT=unconfigured
SAMPLE_HOSTING=unconfigured
SAMPLE_BROWSER=none
declare -A SAMPLE_PROBES=()

usage() {
  cat <<'HELP'
Jelly CLI demo
  ./scripts/dev.sh demo            Interactive on a terminal
  ./scripts/dev.sh demo --menu     Interactive with terminal or piped input
  ./scripts/dev.sh demo --tour     Pre-filled example journey
  ./scripts/dev.sh demo --scenario setup|browsers|alternatives|manage|tests|recovery
  ./scripts/dev.sh demo --hero     Original Jelly logo

You choose actual menu options and enter sample values. All state is kept in
memory; no Docker/Podman, MCP, network, configuration, filesystem writes,
installers or tests are run. Business process diagrams are in:
  docs/references/diagrams/DEV.SH.md
HELP
}

# Redraw the active page in an interactive terminal; redirected transcripts stay linear.
demo_interactive() {
  [[ -t 0 && -t 1 && "${TERM:-dumb}" != dumb ]]
}
demo_page() {
  if demo_interactive; then
    printf '\033[2J\033[H'
    dev::brand hero
    dev::subchapter 'JELLY  /  INTERACTIVE DEMO'
  fi
}
demo_continue() {
  if demo_interactive; then
    local unused
    printf '\n'
    ask 'Press Enter to continue: ' unused || :
  fi
}
header() {
  if demo_interactive; then
    demo_page
  else
    dev::brand hero
    dev::subchapter 'JELLY  /  INTERACTIVE DEMO'
  fi
  dev::item 'Environment' 'Sample data · changes stay in this session'
}
hero() { dev::brand hero; }

# Print the prompt even when stdin is piped, so --tour is reviewable.
ask() {
  local label="$1" var="$2"
  printf '%b›%b  %s' "$C_HONEY" "$C_RESET" "$label"
  IFS= read -r "$var" || return 1
  # A redirected tour has no terminal echo, so show the selected sample input
  # and keep the following output on a fresh line.
  if [[ ! -t 0 ]]; then printf '%s\n' "${!var}"; fi
}
choose() {
  local title="$1" min="$2" max="$3" value
  while true; do
    ask "$title " value || return 1
    if [[ "$value" =~ ^[0-9]+$ ]] && ((10#$value >= min && 10#$value <= max)); then
      REPLY="$((10#$value))"
      return 0
    fi
    dev::note "Choose a number between $min and $max."
  done
}
yes() {
  local answer
  ask "$1 [y/N]: " answer || return 1
  [[ "$answer" == y || "$answer" == Y || "$answer" == yes || "$answer" == YES ]]
}

status() {
  dev::section 'SYSTEM'
  dev::item 'MCP' "$SAMPLE_SERVICE"
  dev::item 'Client' "$SAMPLE_CLIENT"
  dev::item 'Hosting' "$SAMPLE_HOSTING"
  dev::item 'Browser' "$SAMPLE_BROWSER"
  case "$SAMPLE_ENV" in
    podman) dev::item 'Docker' 'not installed'; dev::item 'Podman' 'available · rootless' ;;
    docker) dev::item 'Docker' 'available · rootful'; dev::item 'Podman' 'not installed' ;;
    both) dev::item 'Docker' 'available · rootful'; dev::item 'Podman' 'available · rootless' ;;
    none) dev::item 'Docker' 'not installed'; dev::item 'Podman' 'not installed' ;;
  esac
}
environment() {
  demo_page
  dev::chapter 'ENVIRONMENT' 'Choose a sample computer'
  dev::option 1 'Podman rootless only'
  dev::option 2 'Docker rootful only'
  dev::option 3 'Both Docker and Podman'
  dev::option 4 'Neither installed'
  dev::option 0 'Back'
  choose 'Sample environment [0-4]:' 0 4 || return 0
  case "$REPLY" in
    1) SAMPLE_ENV=podman ;; 2) SAMPLE_ENV=docker ;;
    3) SAMPLE_ENV=both ;; 4) SAMPLE_ENV=none ;; 0) return 0 ;;
  esac
  SAMPLE_PROBES=()
  dev::success "Sample environment: $SAMPLE_ENV"
}
setup() {
  local client hosting address integration service
  demo_page
  dev::chapter '01' 'Get started · MCP setup'
  dev::subchapter 'STEP 1/4 · Client and authorization'
  dev::option 1 'ChatGPT / remote OAuth client'
  dev::option 2 'Local MCP client with bearer token'
  dev::option 3 'Other remote HTTPS MCP client'
  dev::option 0 'Back'
  choose 'Client [0-3]:' 0 3 || return 0
  [[ "$REPLY" != 0 ]] || return 0
  case "$REPLY" in 1) client='ChatGPT / OAuth' ;; 2) client='Local / bearer' ;; 3) client='Remote / OAuth' ;; esac
  demo_page
  dev::chapter '01' 'Get started · MCP setup'
  dev::subchapter 'STEP 2/4 · Hosting'
  if [[ "$client" == 'Local / bearer' ]]; then
    hosting='Local HTTP'; address='127.0.0.1:8787'
    dev::item 'Connection' "$hosting · $address"
  else
    dev::option 1 'Quick Tunnel · temporary HTTPS URL'
    dev::option 2 'Fixed Cloudflare/domain URL'
    dev::option 3 'nip.io / hostname-based HTTPS'
    dev::option 0 'Back'
    choose 'Hosting [0-3]:' 0 3 || return 0
    [[ "$REPLY" != 0 ]] || return 0
    case "$REPLY" in
      1) hosting='Quick Tunnel'; address='https://sample.trycloudflare.com' ;;
      2)
        hosting='Fixed HTTPS domain'
        ask 'Public HTTPS URL (e.g. https://jelly.example.com): ' address || return 0
        if [[ ! "$address" =~ ^https://[a-zA-Z0-9.-]+(:[0-9]+)?/?$ ]]; then
          dev::note 'Enter an HTTPS URL; no setting changed.'
          return 0
        fi ;;
      3) hosting='nip.io HTTPS'; address='https://192-0-2-10.nip.io' ;;
    esac
  fi
  demo_page
  dev::chapter '01' 'Get started · MCP setup'
  dev::subchapter 'STEP 3/4 · Optional integrations'
  dev::option 1 'Skip Telegram human-in-the-loop'
  dev::option 2 'Enable Telegram human-in-the-loop'
  choose 'Telegram [1-2]:' 1 2 || return 0
  integration=no; [[ "$REPLY" == 2 ]] && integration=yes
  dev::option 1 'Save configuration only'
  dev::option 2 'Save and start Jelly services'
  choose 'After saving [1-2]:' 1 2 || return 0
  service="$REPLY"
  demo_page
  dev::chapter 'REVIEW' 'MCP configuration'
  dev::item 'Client' "$client"
  dev::item 'Authentication' "$([[ "$client" == 'Local / bearer' ]] && echo 'Bearer token generated' || echo 'OAuth configured')"
  dev::item 'Hosting' "$hosting"
  dev::item 'Endpoint' "$address"
  dev::item 'Telegram' "$integration"
  dev::item 'Action' "$([[ "$service" == 1 ]] && echo 'Save only' || echo 'Save and start services')"
  if yes 'Apply these settings?'; then
    SAMPLE_CLIENT="$client"; SAMPLE_HOSTING="$hosting"
    [[ "$service" == 2 ]] && SAMPLE_SERVICE=active
    dev::success 'MCP settings applied.'
    dev::item 'Next' 'Configure a browser and verify CDP.'
  else dev::note 'Cancelled.'; fi
}

available_engines() {
  case "$SAMPLE_ENV" in
    podman) echo podman ;; docker) echo docker ;;
    both) printf '%s\n' docker podman ;; none) : ;;
  esac
}
engine_mode() { case "$1" in docker) echo rootful ;; podman) echo rootless ;; esac; }
example_result() {
  local engine="$1" sandbox="$2" security="$3" network="$4"
  if [[ "$engine" == podman && "$network" == private ]]; then
    echo 'FAIL · host CDP unreachable without loopback publication'; return
  fi
  if [[ "$sandbox" == disabled ]]; then echo PASS; return; fi
  if [[ "$security" == restricted ]]; then echo 'FAIL · zygote / namespace'; return; fi
  [[ "$engine" == podman ]] && echo PASS || echo 'FAIL · namespace creation'
}
probe_key() { printf '%s' "$1|$2|$3|$4|$5"; }

engine_choice() {
  local -a engines=()
  local current i=0
  while IFS= read -r current; do [[ -n "$current" ]] && engines+=("$current"); done < <(available_engines)
  if (("${#engines[@]}" == 0)); then
    dev::note 'No container engine available; choose native or external Chromium.'
    return 1
  fi
  for current in "${engines[@]}"; do i=$((i+1)); dev::option "$i" "$current · $(engine_mode "$current")"; done
  dev::option 0 'Back'
  choose "Engine [0-$i]:" 0 "$i" || return 1
  [[ "$REPLY" != 0 ]] || return 1
  ENGINE="${engines[$((REPLY-1))]}"
}
security_and_network() {
  dev::subchapter 'STEP 1/2 · Security'
  dev::option 1 'Sandbox enabled'
  dev::option 2 'Sandbox disabled'
  choose 'Chromium sandbox [1-2]:' 1 2 || return 1
  SANDBOX=enabled; [[ "$REPLY" == 2 ]] && SANDBOX=disabled
  dev::option 1 'Restricted · no-new-privileges, drop capabilities'
  dev::option 2 'Standard · engine defaults'
  choose 'Engine restrictions [1-2]:' 1 2 || return 1
  SECURITY=restricted; [[ "$REPLY" == 2 ]] && SECURITY=standard
  demo_page
  dev::chapter '02' 'Configure browsers'
  dev::subchapter 'STEP 2/2 · Profile and network'
  dev::option 1 'Isolated profile'
  dev::option 2 'Shared downloads'
  dev::option 3 'Ephemeral profile'
  choose 'Profile [1-3]:' 1 3 || return 1
  case "$REPLY" in 1) PROFILE=isolated ;; 2) PROFILE=shared ;; 3) PROFILE=ephemeral ;; esac
  dev::option 1 'Private bridge · no published host port'
  dev::option 2 'Loopback publication · 127.0.0.1'
  choose 'CDP exposure [1-2]:' 1 2 || return 1
  NETWORK=private; [[ "$REPLY" == 2 ]] && NETWORK=loopback
}
review_browser() {
  local outcome key
  demo_page
  dev::chapter 'REVIEW' 'Browser deployment'
  dev::item 'Engine' "$ENGINE · $(engine_mode "$ENGINE")"
  dev::item 'Sandbox' "$SANDBOX"
  dev::item 'Restrictions' "$SECURITY"
  dev::item 'Profile' "$PROFILE"
  dev::item 'CDP exposure' "$NETWORK"
  key="$(probe_key "$ENGINE" "$SANDBOX" "$SECURITY" "$PROFILE" "$NETWORK")"
  dev::item 'Demo probe' "${SAMPLE_PROBES[$key]:-not yet tested}"
  outcome="$(example_result "$ENGINE" "$SANDBOX" "$SECURITY" "$NETWORK")"
  dev::item 'Example outcome' "$outcome"
  if yes 'Create and register this browser?'; then
    if [[ "$outcome" == PASS ]]; then
      SAMPLE_BROWSER="$ENGINE · $SANDBOX · $SECURITY · $PROFILE"
      dev::success 'Browser registered.'
      dev::item 'Next' 'Verify browser routing in MCP.'
    else
      dev::check_missing "Browser could not start: $outcome"
      dev::item 'Next' 'Inspect diagnostics or choose other settings.'
    fi
  else dev::note 'Cancelled.'; fi
}
advanced_browser() {
  local ENGINE SANDBOX SECURITY PROFILE NETWORK
  demo_page
  dev::chapter 'ADVANCED' 'All container settings'
  engine_choice || return 0
  security_and_network || return 0
  review_browser
}
probe() {
  local ENGINE SANDBOX SECURITY PROFILE NETWORK outcome key
  demo_page
  dev::chapter 'PROBE' 'Test a browser configuration'
  engine_choice || return 0
  security_and_network || return 0
  demo_page
  dev::chapter 'REVIEW' 'CDP probe'
  dev::item 'Engine' "$ENGINE · $(engine_mode "$ENGINE")"
  dev::item 'Sandbox' "$SANDBOX"
  dev::item 'Restrictions' "$SECURITY"
  dev::item 'Profile' "$PROFILE"
  dev::item 'CDP exposure' "$NETWORK"
  if yes 'Test this combination?'; then
    outcome="$(example_result "$ENGINE" "$SANDBOX" "$SECURITY" "$NETWORK")"
    key="$(probe_key "$ENGINE" "$SANDBOX" "$SECURITY" "$PROFILE" "$NETWORK")"
    SAMPLE_PROBES["$key"]="$outcome"
    if [[ "$outcome" == PASS ]]; then dev::check_ok 'CDP PASS · example'; else dev::check_missing "$outcome"; fi
    dev::item 'Recorded' 'Current demo session'
  else dev::note 'Cancelled.'; fi
}
recommended() {
  local engine selected ENGINE SANDBOX SECURITY PROFILE NETWORK
  demo_page
  dev::chapter 'RECOMMENDED' 'Candidates and alternatives'
  if [[ "$SAMPLE_ENV" == none ]]; then
    dev::note 'No container engines. Native and external Chromium remain options.'
    return 0
  fi
  for engine in $(available_engines); do dev::item 'Available' "$engine · $(engine_mode "$engine")"; done
  if [[ "$SAMPLE_ENV" == docker ]]; then
    dev::option 1 'Docker rootful · sandbox disabled, restricted'
    dev::option 2 'Docker rootful · sandbox disabled, standard'
  else
    dev::option 1 'Podman rootless · sandbox enabled, standard'
    dev::option 2 'Podman rootless · sandbox disabled, restricted'
  fi
  dev::option 3 'Any combination (advanced)'
  dev::option 0 'Back'
  choose 'Configuration [0-3]:' 0 3 || return 0
  selected="$REPLY"
  [[ "$selected" != 0 ]] || return 0
  [[ "$selected" != 3 ]] || { advanced_browser; return; }
  ENGINE=podman; SANDBOX=enabled; SECURITY=standard
  if [[ "$SAMPLE_ENV" == docker ]]; then
    ENGINE=docker; SANDBOX=disabled; SECURITY=restricted
    [[ "$selected" == 2 ]] && SECURITY=standard
  elif [[ "$selected" == 2 ]]; then
    SANDBOX=disabled; SECURITY=restricted
  fi
  dev::subchapter 'PROFILE AND NETWORK'
  dev::option 1 'Isolated profile'
  dev::option 2 'Shared downloads'
  dev::option 3 'Ephemeral profile'
  choose 'Profile [1-3]:' 1 3 || return 0
  case "$REPLY" in 1) PROFILE=isolated ;; 2) PROFILE=shared ;; 3) PROFILE=ephemeral ;; esac
  dev::option 1 'Private bridge'
  dev::option 2 'Loopback 127.0.0.1'
  choose 'CDP exposure [1-2]:' 1 2 || return 0
  NETWORK=private; [[ "$REPLY" == 2 ]] && NETWORK=loopback
  review_browser
}
native_browser() {
  demo_page
  dev::chapter 'REVIEW' 'Native Chromium'
  dev::item 'Backend' 'Host Chromium'
  if yes 'Use native Chromium?'; then SAMPLE_BROWSER=native; dev::success 'Native Chromium selected.'; else dev::note 'Cancelled.'; fi
}
external_browser() {
  local address
  demo_page
  dev::chapter 'REVIEW' 'Existing remote Chromium'
  ask 'CDP address (hostname:port or IP:port): ' address || return 0
  if [[ ! "$address" =~ ^[a-zA-Z0-9.-]+:[0-9]{1,5}$ ]]; then dev::note 'Enter hostname:port or IP:port.'; return 0; fi
  dev::item 'Address' "$address"
  dev::note 'Only trusted clients should be able to reach CDP.'
  if yes 'Register this external browser?'; then
    SAMPLE_BROWSER="external · $address"; dev::success 'External browser registered.'
  else dev::note 'Cancelled.'; fi
}
browser_menu() {
  while true; do
    demo_page
    dev::chapter '02' 'Configure browsers'
    dev::item 'Engine availability' "$SAMPLE_ENV"
    dev::option 1 'Recommended configurations'
    dev::option 2 'Test a configuration'
    dev::option 3 'Advanced container options'
    dev::option 4 'Native Chromium'
    dev::option 5 'External Chromium'
    dev::option 6 'Change sample computer'
    dev::option 0 'Back'
    choose 'Browser [0-6]:' 0 6 || return 0
    case "$REPLY" in
      1) recommended ;; 2) probe ;; 3) advanced_browser ;;
      4) native_browser ;; 5) external_browser ;; 6) environment ;; 0) return 0 ;;
    esac
    demo_continue
  done
}
manage() {
  while true; do
    demo_page
    dev::chapter '03' 'Manage Jelly'
    dev::item 'MCP service' "$SAMPLE_SERVICE"
    dev::option 1 'Show status'
    dev::option 2 'Show logs'
    dev::option 3 'Start services'
    dev::option 4 'Stop services'
    dev::option 0 'Back'
    choose 'Manage [0-4]:' 0 4 || return 0
    case "$REPLY" in
      1) status ;;
      2) dev::subchapter 'RECENT LOGS'; dev::item '12:01:00' 'MCP listener ready'; dev::item '12:01:04' 'CDP session connected' ;;
      3) if yes 'Start MCP and browser services?'; then SAMPLE_SERVICE=active; dev::success 'Services started.'; else dev::note 'Cancelled.'; fi ;;
      4) if yes 'Stop MCP and browser services?'; then SAMPLE_SERVICE=stopped; dev::success 'Services stopped.'; else dev::note 'Cancelled.'; fi ;;
      0) return 0 ;;
    esac
    demo_continue
  done
}
browser_integration() {
  demo_page
  dev::chapter '04' 'Browser integration · CDP and MCP'
  dev::item 'Sample MCP service' "$SAMPLE_SERVICE"
  dev::item 'Selected browser' "$SAMPLE_BROWSER"
  dev::option 1 'Run against the current sample configuration'
  dev::option 2 'Explore a successful integration test'
  dev::option 3 'Explore a failing test and its diagnostics'
  dev::option 0 'Back'
  choose 'Integration [0-3]:' 0 3 || return 0
  case "$REPLY" in
    1)
      if [[ "$SAMPLE_BROWSER" == none ]]; then
        dev::note 'NOT RUN · No browser has been selected.'
        dev::item 'Next' 'Configure a browser before testing MCP routing.'
      elif [[ "$SAMPLE_SERVICE" != active ]]; then
        dev::note 'NOT RUN · The sample MCP service is stopped.'
        dev::item 'Next' 'Start the sample service in Manage Jelly.'
      else
        dev::check_ok 'Browser integration PASS (example)'
        dev::item 'CDP' 'Browser endpoint responded'
        dev::item 'MCP' 'Registered browser selected successfully'
        dev::item 'Scope' 'CDP handshake and MCP routing'
      fi ;;
    2)
      dev::check_ok 'Browser integration PASS (example)'
      dev::item 'Browser' 'Chromium ready · CDP WebSocket reachable'
      dev::item 'MCP' 'Browser registration and routing verified'
      dev::item 'Scope' 'Example run, not a local test result' ;;
    3)
      dev::check_missing 'Browser integration FAIL (example)'
      dev::item 'Observed' 'Chromium exited before CDP became ready'
      dev::item 'Cause' 'Namespace creation denied by the container engine'
      dev::item 'Next' 'Inspect Chromium logs or explicitly select different settings' ;;
    0) return 0 ;;
  esac
}
tests() {
  while true; do
    demo_page
    dev::chapter '04' 'Tests and verification'
    dev::option 1 'Project checks · lint and CLI contracts'
    dev::option 2 'Rust unit tests'
    dev::option 3 'Browser integration'
    dev::option 4 'Catalog and acceptance coverage'
    dev::option 0 'Back'
    choose 'Test [0-4]:' 0 4 || return 0
    case "$REPLY" in
      1) dev::check_ok 'Checks passed (example)'; dev::item 'Scope' 'Docs · lint · CLI contracts' ;;
      2) dev::check_ok 'Rust tests passed (example)'; dev::item 'Results' '272 passed · 1 ignored' ;;
      3) browser_integration ;;
      4) dev::item 'Coverage' 'Test IDs and acceptance criteria'; dev::item 'Evidence' 'Catalog does not prove execution' ;;
      0) return 0 ;;
    esac
    demo_continue
  done
}
recovery() {
  demo_page
  dev::chapter '05' 'Failure and recovery'
  dev::check_missing 'CDP endpoint not ready (example)'
  dev::item 'Engine' "$SAMPLE_ENV"
  dev::item 'Next' 'Check namespace permissions, sandbox and profile ownership'
  dev::option 1 'View example logs'
  dev::option 2 'Try a different browser'
  dev::option 0 'Back'
  choose 'Recovery [0-2]:' 0 2 || return 0
  case "$REPLY" in
    1) dev::item 'Chromium' 'Failed to move to new namespace: Operation not permitted' ;;
    2) browser_menu ;;
    0) : ;;
  esac
}
main_menu() {
  if ! demo_interactive; then header; fi
  while true; do
    demo_page
    status
    dev::chapter 'JELLY' 'What would you like to do?'
    dev::option 1 'Get started · MCP setup'
    dev::option 2 'Configure browsers'
    dev::option 3 'Connect an external browser'
    dev::option 4 'Manage Jelly'
    dev::option 5 'Tests and verification'
    dev::option 6 'Change sample computer'
    dev::option 7 'Failure and recovery'
    dev::option 0 'Exit'
    choose 'Choice [0-7]:' 0 7 || return 0
    case "$REPLY" in
      1) setup; demo_continue ;;
      2) browser_menu ;;
      3) external_browser; demo_continue ;;
      4) manage ;;
      5) tests ;;
      6) environment; demo_continue ;;
      7) recovery; demo_continue ;;
      0) return 0 ;;
    esac
  done
}
tour() {
  header
  status
  setup <<< $'1\n1\n1\n2\ny'
  browser_menu <<< $'1\n1\n1\n2\ny\n0'
  manage <<< $'1\n0'
  tests <<< $'2\n0'
  recovery <<< $'1'
  dev::chapter 'FINISH' 'Sample session'
  dev::item 'MCP' "$SAMPLE_SERVICE"
  dev::item 'Browser' "$SAMPLE_BROWSER"
}
if (($# == 0)); then
  if [[ -t 0 && -t 1 ]]; then main_menu; else tour; fi
  exit 0
fi
case "$1" in
  --menu) (($# == 1)) || { usage >&2; exit 2; }; main_menu ;;
  --tour) (($# == 1)) || { usage >&2; exit 2; }; tour ;;
  --hero) (($# == 1)) || { usage >&2; exit 2; }; hero ;;
  --scenario)
    (($# == 2)) || { usage >&2; exit 2; }
    header
    case "$2" in
      setup) setup ;; browsers) browser_menu ;;
      alternatives) SAMPLE_ENV=none; browser_menu ;;
      manage) manage ;; tests) tests ;; recovery) recovery ;;
      *) usage >&2; exit 2 ;;
    esac ;;
  --help|-h) (($# == 1)) || { usage >&2; exit 2; }; usage ;;
  *) usage >&2; exit 2 ;;
esac
