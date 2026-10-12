#!/usr/bin/env bash
# Chromium containers for Jelly. Network exposure and sandboxing are operator choices.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# Share the same branding and prompt vocabulary as the setup and home screens.
# shellcheck source=../lib/runtime.sh
source "$ROOT/scripts/lib/runtime.sh"
dev::ui_init
NETWORK="jelly-browsers"
IMAGE="localhost/jelly-chromium:local"

usage() {
  cat <<'HELP'
Usage: ./scripts/dev.sh browser [menu|detect|recommend|probe|add|list|stop] [options]
  add --engine docker|podman --profile isolated|shared|ephemeral [--sandbox enabled|disabled] [--publish IP:PORT] [--engine-security restricted|standard] [--build-network default|host] [--bridge-dns auto|enabled|disabled]
  detect                            Show accessible engines (read-only)
  recommend                         Show local CDP evidence and reference candidates (read-only)
  probe --engine docker|podman --sandbox enabled|disabled --engine-security restricted|standard [--profile isolated|shared|ephemeral] [--publish IPv4:PORT]
                                     Test temporary Chromium/CDP; does not register browser
  list --engine docker|podman
  stop --engine docker|podman --container jelly-chromium-N
  native                            Switch Jelly back to native browser mode
  connect --address HOST:PORT       Register externally hosted Chromium CDP
  diagnose --engine ENGINE --container NAME   Inspect requested/observed security
  menu                              Interactive browser setup
Without --publish: Linux rootful bridge networking with no published CDP port.
With --publish IP:PORT: the operator explicitly selects a host bind address.
HELP
}

action="${1:-menu}"; (($#)) && shift
engine=docker profile=isolated container="" address="" sandbox=disabled publish="" engine_security=restricted build_network=default bridge_dns=auto
while (($#)); do
  case "$1" in
    --engine) engine="${2:?missing engine}"; shift 2;;
    --profile) profile="${2:?missing profile}"; shift 2;;
    --container) container="${2:?missing container}"; shift 2;;
    --address) address="${2:?missing address}"; shift 2;;
    --sandbox) sandbox="${2:?missing sandbox}"; shift 2;;
    --publish) publish="${2:?missing publish}"; shift 2;;
    --engine-security) engine_security="${2:?missing engine security}"; shift 2;;
    --build-network) build_network="${2:?missing build network}"; shift 2;;
    --bridge-dns) bridge_dns="${2:?missing bridge dns}"; shift 2;;
    *) break;;
  esac
done
(($# == 0)) || { usage >&2; exit 2; }
validate_choices() {
[[ "$engine" == docker || "$engine" == podman ]] || { echo 'engine must be docker or podman' >&2; return 2; }
[[ "$profile" == isolated || "$profile" == shared || "$profile" == ephemeral ]] || { echo 'invalid browser profile' >&2; return 2; }
[[ "$sandbox" == enabled || "$sandbox" == disabled ]] || { echo 'sandbox must be enabled or disabled' >&2; return 2; }
[[ "$engine_security" == restricted || "$engine_security" == standard ]] || { echo 'engine security must be restricted or standard' >&2; return 2; }
[[ "$build_network" == default || "$build_network" == host ]] || { echo 'build network must be default or host' >&2; return 2; }
[[ "$bridge_dns" == auto || "$bridge_dns" == enabled || "$bridge_dns" == disabled ]] || { echo 'bridge dns must be auto, enabled or disabled' >&2; return 2; }
if [[ "$engine" == docker && "$bridge_dns" == disabled ]]; then echo '--bridge-dns disabled is supported for Podman only' >&2; return 2; fi
if [[ -n "$publish" ]]; then
  if [[ ! "$publish" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}:[0-9]{1,5}$ ]]; then
    echo '--publish requires IPv4:PORT' >&2; return 2
  fi
  port_check="${publish##*:}"
  ((10#$port_check >= 1 && 10#$port_check <= 65535)) || { echo 'port must be between 1 and 65535' >&2; return 2; }
  IFS=. read -r o1 o2 o3 o4 <<< "${publish%:*}"
  for octet in "$o1" "$o2" "$o3" "$o4"; do
    ((10#$octet <= 255)) || { echo 'invalid IPv4 address' >&2; return 2; }
  done
fi

}
validate_choices || exit 2

runtime() {
  command -v "$engine" >/dev/null || { echo "$engine is not installed" >&2; exit 1; }
  "$engine" info >/dev/null || { echo "$engine daemon/service unavailable" >&2; exit 1; }
}

next_container() {
  local n=1
  while "$engine" container inspect "jelly-chromium-$n" >/dev/null 2>&1; do
    ((n+=1))
  done
  printf 'jelly-chromium-%s' "$n"
}

# Read-only engine detection and local evidence. No sudo, daemon start or installation.
engine_mode() {
  local selected="$1" opts
  if [[ "$selected" == docker ]]; then
    opts="$(docker info --format '{{json .SecurityOptions}}' 2>/dev/null)" || return 1
    [[ "$opts" == *name=rootless* ]] && echo rootless || echo rootful
  else
    opts="$(podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null)" || return 1
    [[ "$opts" == true ]] && echo rootless || echo rootful
  fi
}

detect_engines() {
  local selected mode found=false
  dev::section 'ENGINE AVAILABILITY'
  for selected in docker podman; do
    if ! command -v "$selected" >/dev/null 2>&1; then
      dev::item "$selected" 'Not installed'
    elif mode="$(engine_mode "$selected")"; then
      dev::item "$selected" "Available · $mode"
      found=true
    else
      dev::item "$selected" 'Installed · no socket access'
    fi
  done
  if ! $found; then
    dev::note 'Native and external Chromium still work without a container engine.'
  fi
}

probe_cache_home() {
  printf '%s/jelly/browser-probes' "${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}"
}
probe_cache_key() {
  local mode version image_id source_id os_id exposure
  # Local CDP evidence must not stand in for LAN routing or shared download mounts.
  [[ "$profile" != shared ]] || return 1
  case "$publish" in
    "") exposure=subnet ;;
    127.0.0.1:*) exposure=loopback ;;
    *) return 1 ;;
  esac
  mode="$(engine_mode "$engine" 2>/dev/null)" || return 1
  version="$("$engine" --version 2>/dev/null)" || return 1
  image_id="$("$engine" image inspect "$IMAGE" --format '{{.Id}}' 2>/dev/null)" || image_id=missing
  source_id="$(sha256sum "$ROOT/containers/chromium/Containerfile" "$ROOT/containers/chromium/entrypoint.sh" | sha256sum | cut -d' ' -f1)"
  os_id="$(cat /etc/os-release 2>/dev/null | sha256sum | cut -d' ' -f1)"
  printf '%s\n' "$engine|$mode|$version|$(uname -r)|$os_id|$image_id|$source_id|$profile|$sandbox|$engine_security|$build_network|$bridge_dns|$exposure" | sha256sum | cut -d' ' -f1
}
probe_cached_status() {
  local key path status
  key="$(probe_cache_key)" || { echo UNVERIFIED; return; }
  path="$(probe_cache_home)/$key"
  if [[ -f "$path" ]]; then
    IFS=$'\t' read -r status _ < "$path" || :
    case "$status" in PASS|FAIL) echo "$status"; return;; esac
  fi
  echo UNVERIFIED
}
save_probe_status() {
  local status="$1" key dir
  key="$(probe_cache_key)" || return 0
  dir="$(probe_cache_home)"
  (umask 077; mkdir -p "$dir"; chmod 700 "$dir"; printf '%s\t%s\n' "$status" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$dir/$key") || echo 'WARNING: failed to store local probe result' >&2
}

reference_observation() {
  if [[ "$engine" == podman && "$sandbox" == enabled && "$engine_security" == standard ]]; then
    echo 'reference PASS (Arch test, NOT locally verified)'
  elif [[ "$sandbox" == disabled ]]; then
    echo 'reference PASS (Arch test, sandbox disabled)'
  else
    echo 'reference FAIL (Arch test, may differ on this host)'
  fi
}

declare -a CHOICE_ENGINES=() CHOICE_SANDBOXES=() CHOICE_RESTRICTIONS=()
recommend() {
  local selected mode s sec status reference phase row kind sb sr st ref count=0
  local -a rows=()
  CHOICE_ENGINES=(); CHOICE_SANDBOXES=(); CHOICE_RESTRICTIONS=()
  echo 'Local CDP evidence (NOT a Chromium sandbox attestation):'
  for selected in docker podman; do
    command -v "$selected" >/dev/null 2>&1 || continue
    mode="$(engine_mode "$selected")" || continue
    for s in enabled disabled; do
      for sec in restricted standard; do
        engine="$selected"; sandbox="$s"; engine_security="$sec"; profile=isolated
        build_network=default; bridge_dns=auto; publish=""
        [[ "$mode" == rootless ]] && publish=127.0.0.1:0
        status="$(probe_cached_status)"
        reference="$(reference_observation)"
        rows+=("$selected|$mode|$s|$sec|$status|$reference")
      done
    done
  done
  if (("${#rows[@]}" == 0)); then detect_engines; return 0; fi
  echo 'PASS means observed Chromium/CDP on this configuration; reference results are not local PASS.'
  for phase in PASS REF_PASS FAIL OTHER; do
    case "$phase" in
      PASS) echo '  Locally verified (CDP):';;
      REF_PASS) echo '  Reference PASS, not yet verified on this installation:';;
      FAIL) echo '  Local CDP FAIL (still selectable):';;
      OTHER) echo '  Other advanced combinations (reference failed; still selectable):';;
    esac
    for row in "${rows[@]}"; do
      IFS='|' read -r selected kind sb sr st ref <<< "$row"
      case "$phase" in
        PASS) [[ "$st" == PASS ]] || continue ;;
        REF_PASS) [[ "$st" == UNVERIFIED && "$ref" == reference\ PASS* ]] || continue ;;
        FAIL) [[ "$st" == FAIL ]] || continue ;;
        OTHER) [[ "$st" == UNVERIFIED && "$ref" != reference\ PASS* ]] || continue ;;
      esac
      count=$((count + 1))
      CHOICE_ENGINES+=("$selected"); CHOICE_SANDBOXES+=("$sb"); CHOICE_RESTRICTIONS+=("$sr")
      printf '  %b%2d%b  %s · %s  sandbox=%s  security=%s\n' \
        "$C_HONEY$C_BOLD" "$count" "$C_RESET" "$selected" "$kind" "$sb" "$sr"
      if [[ "$st" == PASS ]]; then
        printf '      %b%s%b\n' "$C_GREEN$C_BOLD" 'LOCAL CDP PASS' "$C_RESET"
      elif [[ "$st" == FAIL ]]; then
        printf '      %b%s%b · %s\n' "$C_RED" 'LOCAL CDP FAIL' "$C_RESET" "$ref"
      else
        printf '      %b%s%b · %s\n' "$C_DIM" 'UNVERIFIED' "$C_RESET" "$ref"
      fi
    done
  done
  echo 'Use browser probe to test a choice or browser add to create one explicitly.'
  echo 'Evidence covers isolated-profile CDP and selected subnet/loopback mode, not downloads or sandbox attestation.'
}

ensure_bridge_network() {
  local chosen_network="$1"
  if ! "$engine" network inspect "$chosen_network" >/dev/null 2>&1; then
    local -a network_args=(network create --driver bridge)
    if [[ "$engine" == podman ]]; then
      local dns_choice="$bridge_dns"
      if [[ "$dns_choice" == auto ]]; then
        dns_choice=enabled
        if [[ "$(id -u)" != 0 && -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then dns_choice=disabled; fi
      fi
      if [[ "$dns_choice" == disabled ]]; then
        echo 'NOTICE: Podman bridge DNS plugin disabled (container-name DNS unavailable).'
        network_args+=(--disable-dns)
      fi
    fi
    "$engine" "${network_args[@]}" "$chosen_network" >/dev/null
  fi
}

build_chromium_image() {
  local -a build_args=(build -q)
  if [[ "$build_network" == host ]]; then
    echo 'NOTICE: operator-selected host networking applies only to image build steps, not runtime networking.'
    build_args+=(--network host)
  fi
  "$engine" "${build_args[@]}" -t "$IMAGE" -f "$ROOT/containers/chromium/Containerfile" "$ROOT/containers/chromium" >/dev/null
}

# An explicit, disposable real-CDP probe. It never updates Jelly browser config.
probe() (
  validate_choices || return 2
  runtime
  command -v curl >/dev/null || { echo 'curl is required for CDP probes' >&2; exit 1; }
  command -v jq >/dev/null || { echo 'jq is required for CDP probes' >&2; exit 1; }
  local tmp name net pname dname ip endpoint port mapping status=FAIL
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/jelly-chromium-probe.XXXXXXXX")"
  name="jelly-probe-$(basename "$tmp" | tr '[:upper:]' '[:lower:]' | tr '.' '-')"
  net="${name}-net"; pname="${name}-profile"; dname="${name}-downloads"
  probe_cleanup() {
    "$engine" rm -f "$name" >/dev/null 2>&1 || :
    "$engine" volume rm "$pname" "$dname" >/dev/null 2>&1 || :
    "$engine" network rm "$net" >/dev/null 2>&1 || :
    rm -rf -- "$tmp"
  }
  trap probe_cleanup EXIT
  echo "Testing engine=$engine sandbox=$sandbox restrictions=$engine_security profile=$profile (without browser registration)."
  if [[ -z "$publish" && "$engine" == podman && "$(engine_mode podman)" == rootless ]]; then
    echo 'Rootless Podman needs an explicitly selected --publish 127.0.0.1:PORT for host-CDP testing.' >&2
    exit 2
  fi
  build_chromium_image || { echo 'Image build failed; no local CDP PASS.' >&2; exit 1; }
  ensure_bridge_network "$net" || { echo 'Could not create isolated probe bridge.' >&2; exit 1; }
  local -a args=(run -d --name "$name" --network "$net" --label jelly.probe=1 --env "JELLY_CHROMIUM_SANDBOX=$sandbox")
  [[ "$engine_security" == restricted ]] && args+=(--security-opt no-new-privileges --cap-drop ALL)
  if [[ -n "$publish" ]]; then
    echo 'Probe uses only an automatically allocated 127.0.0.1 port; requested LAN/port publication is NOT tested.'
    args+=(-p 127.0.0.1::9222)
  fi
  if [[ "$profile" == ephemeral ]]; then
    args+=(--tmpfs /data/profile:rw,nosuid,nodev,size=512m,mode=1777 --tmpfs /browser-downloads:rw,nosuid,nodev,size=512m,mode=1777)
  else
    "$engine" volume create "$pname" >/dev/null || exit 1
    "$engine" volume create "$dname" >/dev/null || exit 1
    args+=(-v "$pname:/data/profile" -v "$dname:/browser-downloads")
    [[ "$profile" == shared ]] && echo 'NOTICE: shared-download host bind mount is NOT tested by this CDP probe.'
  fi
  if ! "$engine" "${args[@]}" "$IMAGE" >"$tmp/id" 2>"$tmp/launch.log"; then
    echo 'Container launch rejected; no alternative settings attempted.' >&2
    tail -n 12 "$tmp/launch.log" >&2
    save_probe_status FAIL
    exit 1
  fi
  if [[ -n "$publish" ]]; then
    mapping="$("$engine" port "$name" 9222/tcp | tail -1)" || mapping=""
    port="${mapping##*:}"
    if [[ ! "$port" =~ ^[0-9]{1,5}$ ]]; then
      echo 'Cannot discover temporary loopback port.' >&2; save_probe_status FAIL; exit 1
    fi
    endpoint="127.0.0.1:$port"
  else
    ip="$("$engine" inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$name")"
    endpoint="$ip:9222"
  fi
  echo "Checking temporary CDP at $endpoint ..."
  for _ in $(seq 1 18); do
    if curl --noproxy '*' --max-time 1 -fsS "http://$endpoint/json/version" > "$tmp/cdp.json" 2>/dev/null &&
       jq -e '(.Browser|type=="string") and (.webSocketDebuggerUrl|type=="string" and startswith("ws://"))' "$tmp/cdp.json" >/dev/null 2>&1; then
      status=PASS
      break
    fi
    [[ "$("$engine" inspect -f '{{.State.Running}}' "$name" 2>/dev/null || echo false)" == true ]] || break
    sleep 1
  done
  save_probe_status "$status"
  if [[ "$status" == PASS ]]; then
    if [[ -n "$publish" && "$publish" != 127.0.0.1:* ]]; then
      echo "LOOPBACK CDP PASS ($engine; sandbox=$sandbox restrictions=$engine_security). Requested LAN publication $publish: UNVERIFIED."
    else
      echo "LOCAL CDP PASS ($engine; sandbox=$sandbox restrictions=$engine_security profile=$profile). $(jq -r .Browser "$tmp/cdp.json")."
    fi
    echo 'Effective sandbox: UNVERIFIED. No Jelly registration or download-transfer test.'
    return 0
  fi
  echo "LOCAL CDP FAIL ($engine; sandbox=$sandbox restrictions=$engine_security). Settings unchanged." >&2
  "$engine" logs --tail 18 "$name" >&2 || :
  echo 'Suggested diagnostics only: inspect kernel namespaces/seccomp or choose other settings explicitly.' >&2
  return 1
)

add() {
  validate_choices || return 2
  runtime
  if [[ -z "$publish" && "$engine" == podman ]] && [[ "$(id -u)" != 0 ]]; then
    echo 'Podman rootless cannot reliably expose bridge IPs to host Jelly. Use rootful Podman or connect through a loopback-only tunnel.' >&2
    exit 1
  fi
  if [[ -z "$publish" && "$(uname -s)" != Linux ]]; then
    echo 'Direct bridge IP connections require Linux. Docker Desktop needs a different secure transport.' >&2
    exit 1
  fi
  ensure_bridge_network "$NETWORK"
  build_chromium_image
  local name ip output
  name="$(next_container)"
  local exposure=subnet
  [[ -n "$publish" ]] && exposure=published
  local -a args=(run -d --name "$name" --network "$NETWORK" --label jelly.browser=1 \
    --label "jelly.security.sandbox=$sandbox" --label "jelly.security.engine=$engine_security" \
    --label "jelly.connection.exposure=$exposure" --label "jelly.browser.profile=$profile" \
    --env "JELLY_CHROMIUM_SANDBOX=$sandbox")
  if [[ "$engine_security" == restricted ]]; then args+=(--security-opt no-new-privileges --cap-drop ALL); fi
  if [[ -n "$publish" ]]; then
    echo "SECURITY: publishing Chromium CDP at $publish grants extensive browser control to anyone who can reach that address."
    args+=(-p "$publish:9222")
  fi
  if [[ "$profile" == ephemeral ]]; then
    args+=(--rm --tmpfs /data/profile:rw,nosuid,nodev,size=512m,mode=1777 --tmpfs /browser-downloads:rw,nosuid,nodev,size=512m,mode=1777)
  else
    args+=(-v "jelly-profile-$name:/data/profile")
    if [[ "$profile" == shared ]]; then
      mkdir -p "$ROOT/.jelly-downloads/$name"
      # The image runs as UID 10001. Rootless Podman maps that UID to a host subuid;
      # inspect ownership inside its user namespace rather than comparing host UIDs.
      local shared_owner ownership_hint
      if [[ "$engine" == podman && "$(id -u)" != 0 ]]; then
        shared_owner="$(podman unshare stat -c %u "$ROOT/.jelly-downloads/$name")"
        ownership_hint="podman unshare chown 10001:10001 '$ROOT/.jelly-downloads/$name'"
      else
        shared_owner="$(stat -c %u "$ROOT/.jelly-downloads/$name")"
        ownership_hint="sudo chown 10001:10001 '$ROOT/.jelly-downloads/$name'"
      fi
      if [[ "$shared_owner" != 10001 ]]; then
        echo "Shared downloads require container-visible UID 10001 ownership of $ROOT/.jelly-downloads/$name" >&2
        echo "Run: $ownership_hint" >&2
        return 1
      fi
      args+=(-v "$ROOT/.jelly-downloads/$name:/browser-downloads")
    else
      args+=(-v "jelly-downloads-$name:/browser-downloads")
    fi
  fi
  if ! "$engine" "${args[@]}" "$IMAGE" >/dev/null; then
    echo 'Container engine rejected the requested configuration; Jelly did not modify your security choices.' >&2
    if [[ "$sandbox" == enabled ]]; then
      echo 'Suggestions: check unprivileged user namespace and seccomp support; try --engine-security standard, or explicitly select --sandbox disabled if appropriate for your environment.' >&2
    fi
    echo "Inspection: $engine info; $engine network inspect $NETWORK" >&2
    return 1
  fi
  ip="$("$engine" inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$name")"
  if [[ -z "$ip" && -z "$publish" ]]; then echo 'No bridge IP; choose --publish IP:PORT or inspect engine networking' >&2; exit 1; fi
  echo "Container: $name ($engine, $profile); bridge address: $ip; sandbox=$sandbox; engine-security=$engine_security; published=${publish:-no}"
  local endpoint_host="$ip" port=9222
  if [[ -n "$publish" ]]; then
    endpoint_host="${publish%:*}"; port="${publish##*:}"
    [[ "$endpoint_host" == 0.0.0.0 ]] && endpoint_host=127.0.0.1
  fi
  echo 'Checking host-to-container CDP connectivity...'
  local ready=false
  for _ in $(seq 1 30); do
    if curl --noproxy '*' -fsS --max-time 1 "http://$endpoint_host:$port/json/version" >/dev/null 2>&1; then ready=true; break; fi
    sleep 1
  done
  if [[ "$ready" != true ]]; then
    echo 'Chromium CDP is not reachable. Container remains available for diagnosis.' >&2
    "$engine" ps -a --filter "name=^/$name$" --format '{{.Names}} {{.Status}}' >&2 || true
    "$engine" logs --tail 35 "$name" >&2 || true
    echo "Suggestion: run ./scripts/dev.sh browser diagnose --engine $engine --container $name" >&2
    if [[ "$sandbox" == enabled ]]; then
      echo 'Suggestion: check kernel user namespaces, seccomp, and setuid sandbox support; try --engine-security standard if permitted, or explicitly choose --sandbox disabled. Jelly will not change your choice.' >&2
    fi
    if [[ -z "$publish" ]]; then
      echo 'Suggestion: check bridge routing from the host; alternatively select --publish 127.0.0.1:PORT.' >&2
    fi
    return 1
  fi
  local connection=subnet
  [[ -n "$publish" ]] && connection=address
  local -a register=(add "http://$endpoint_host:$port" --connection "$connection" --mode remote-only --remote-path /browser-downloads)
  if [[ "$profile" == shared ]]; then
    register=(add "http://$endpoint_host:$port" --connection "$connection" --mode shared-volume --remote-path /browser-downloads --host-path "$ROOT/.jelly-downloads/$name")
  fi
  echo 'Connectivity verified. Chromium sandbox state is NOT attested by a CDP response.'
  if command -v cargo >/dev/null 2>&1; then
    (cd "$ROOT"; cargo run --quiet --locked --bin jelly-browser-connect -- "${register[@]}")
  else
    echo 'Cargo is required to register the browser. The container was created.' >&2
    return 1
  fi
}

list() {
  runtime
  "$engine" ps -a --filter label=jelly.browser=1 --format 'table {{.Names}}\t{{.Status}}\t{{.Networks}}'
  if command -v cargo >/dev/null 2>&1; then
    (cd "$ROOT"; cargo run --quiet --locked --bin jelly-browser-connect -- list)
  fi
}

stop_one() {
  runtime
  [[ "$container" =~ ^jelly-chromium-[0-9]+$ ]] || { echo 'Provide --container jelly-chromium-N' >&2; exit 2; }
  [[ "$("$engine" inspect -f '{{index .Config.Labels "jelly.browser"}}' "$container")" == 1 ]] || {
    echo 'Refusing to stop a container not managed by Jelly browser helper' >&2; exit 1;
  }
  "$engine" stop "$container"
  echo "Stopped $container. Remove its browser registration separately using jelly-browser-connect remove."
}

connect_external() {
  local target="$address"
  if [[ -z "$target" ]]; then
    read -r -p 'Chromium address (IP:PORT, hostname:PORT or ws(s) URL): ' target || return 1
  fi
  [[ -n "$target" ]] || { echo 'An address is required' >&2; return 2; }
  if [[ "$target" != *://* ]]; then target="http://$target"; fi
  command -v cargo >/dev/null 2>&1 || { echo 'Cargo required to register Chromium' >&2; return 1; }
  (cd "$ROOT"; cargo run --quiet --locked --bin jelly-browser-connect -- add "$target" --mode remote-only)
}

diagnose() {
  runtime
  [[ "$container" =~ ^jelly-chromium-[0-9]+$ ]] || { echo 'Provide --container jelly-chromium-N' >&2; return 2; }
  local owner
  owner="$("$engine" inspect -f '{{index .Config.Labels "jelly.browser"}}' "$container")" || return 1
  [[ "$owner" == 1 ]] || { echo 'Not a Jelly-managed Chromium container' >&2; return 1; }
  local requested_sandbox requested_engine exposure running
  requested_sandbox="$("$engine" inspect -f '{{index .Config.Labels "jelly.security.sandbox"}}' "$container")"
  requested_engine="$("$engine" inspect -f '{{index .Config.Labels "jelly.security.engine"}}' "$container")"
  exposure="$("$engine" inspect -f '{{index .Config.Labels "jelly.connection.exposure"}}' "$container")"
  running="$("$engine" inspect -f '{{.State.Running}}' "$container")"
  printf 'Browser: %s\nRequested sandbox: %s\nRequested engine restrictions: %s\nRequested CDP exposure: %s\nObserved container running: %s\n' \
    "$container" "$requested_sandbox" "$requested_engine" "$exposure" "$running"
  printf 'Sandbox effectively active: UNVERIFIED (start flags and connectivity cannot prove sandbox activation)\n'
  if [[ "$running" != true ]]; then
    echo 'Diagnosis: Chromium/container not running. Recent logs:'
    "$engine" logs --tail 35 "$container" || true
    if [[ "$requested_sandbox" == enabled ]]; then
      echo 'Possible remediation: review kernel user namespaces, seccomp, sandbox helper and container restrictions; optionally retry with standard engine restrictions or sandbox disabled.'
    fi
  else
    echo 'Diagnosis: container is running; inspect chrome://sandbox inside the browser for sandbox details, or use a runtime-specific sandbox probe.'
  fi
  echo 'Jelly does not silently change requested security settings.'
}

native() {
  local config="$ROOT/config/jelly.toml"
  if grep -q '^backend = "remote-cdp"$' "$config"; then
    sed -i 's/^backend = "remote-cdp"$/backend = "native"/' "$config"
    echo 'Selected native browser backend. Restart Jelly MCP to apply.'
  else
    echo 'Native browser backend already selected.'
  fi
}

choose_security() {
  local value
  sandbox=disabled; engine_security=restricted
  dev::step '1/2' 'Security choices'
  dev::note 'Sandbox and restrictions are independent. All combinations remain selectable.'
  dev::prompt 'Chromium sandbox [enabled/disabled] (default disabled): ' value || return 1
  sandbox="${value:-disabled}"
  dev::prompt 'Container restrictions [restricted/standard] (default restricted): ' value || return 1
  engine_security="${value:-restricted}"
  choose_deployment
}

choose_deployment() {
  local value
  build_network=default; bridge_dns=auto
  dev::step '2/2' 'Browser profile and connectivity'
  dev::prompt 'Browser profile [isolated/shared/ephemeral] (default isolated): ' value || return 1
  profile="${value:-isolated}"
  if [[ "$(engine_mode "$engine" 2>/dev/null || :)" == rootless ]]; then
    dev::note 'Rootless CDP tests use explicit 127.0.0.1 publication; subnet access is a different test.'
  fi
  dev::prompt 'Publish CDP? IPv4:PORT, or blank for private subnet only: ' value || return 1
  publish="$value"
  dev::prompt 'Image build network [default/host] (default default): ' value || return 1
  build_network="${value:-default}"
  if [[ "$engine" == podman ]]; then
    dev::prompt 'Podman bridge DNS [auto/enabled/disabled] (default auto): ' value || return 1
    bridge_dns="${value:-auto}"
  fi
  if [[ -n "$publish" && "$publish" != 127.0.0.1:* ]]; then
    dev::warning 'CDP may be exposed beyond the local host; check firewall and authorization.'
  fi
  validate_choices
}

review_browser_choice() {
  dev::section 'REVIEW  /  Browser deployment'
  dev::item 'Engine' "$engine ($(engine_mode "$engine" 2>/dev/null || echo unavailable))"
  dev::item 'Profile' "$profile"
  dev::item 'Sandbox requested' "$sandbox"
  dev::item 'Restrictions' "$engine_security"
  dev::item 'CDP exposure' "${publish:-private bridge, no host port}"
  dev::item 'Build network' "$build_network"
  [[ "$engine" == podman ]] && dev::item 'Bridge DNS' "$bridge_dns"
  dev::item 'Local CDP evidence' "$(probe_cached_status)"
  dev::note 'CDP PASS never attests Chromium sandbox or remote downloads.'
  if [[ -n "$publish" && "$publish" != 127.0.0.1:* ]]; then
    dev::warning 'This CDP address may be reachable beyond the local host.'
  fi
  if dev::ask_yes 'Build, create and register this browser?'; then
    if add; then
      dev::success 'Browser created and registered. Restart MCP to load its configuration.'
      dev::note 'Next: ./scripts/dev.sh browser list --engine '"$engine"
    else
      dev::warning 'Browser creation or registration failed; review the diagnostics above.'
      return 1
    fi
  else
    dev::note 'Cancelled; no browser created.'
  fi
}

choose_recommendation() {
  local choice
  recommend
  (("${#CHOICE_ENGINES[@]}" > 0)) || return 0
  read -r -p 'Selection number (blank to cancel): ' choice || return 1
  [[ -n "$choice" ]] || return 0
  if [[ ! "$choice" =~ ^[0-9]+$ ]] || ((10#$choice < 1 || 10#$choice > ${#CHOICE_ENGINES[@]})); then
    echo 'Invalid selection'; return 2
  fi
  local ix=$((10#$choice - 1))
  engine="${CHOICE_ENGINES[$ix]}"
  sandbox="${CHOICE_SANDBOXES[$ix]}"
  engine_security="${CHOICE_RESTRICTIONS[$ix]}"
  echo "Selected explicitly: $engine sandbox=$sandbox restrictions=$engine_security."
  echo 'Profile and network choices are independent and may change whether this configuration works.'
  choose_deployment || return 1
  review_browser_choice
}

choose_probe() {
  local choice
  detect_engines
  read -r -p 'Probe engine [docker/podman] (blank to cancel): ' choice || return 1
  [[ -n "$choice" ]] || return 0
  engine="$choice"
  [[ "$engine" == docker || "$engine" == podman ]] || { echo 'Unknown engine'; return 2; }
  engine_mode "$engine" >/dev/null || { echo 'Engine inaccessible; probe not started.'; return 1; }
  choose_security || return 1
  echo 'An explicit probe builds the image, runs temporary Chromium/CDP, then cleans up.'
  read -r -p 'Run this disposable probe now? [y/N]: ' choice || return 1
  [[ "$choice" =~ ^[yY]$ ]] || return 0
  probe
}

advanced_menu() {
  local choice
  while true; do
    printf '\nAdvanced choices (all configurations remain available)\n'
    printf '  1) Docker: select profile, sandbox, restrictions and network\n'
    printf '  2) Podman: select profile, sandbox, restrictions and network\n'
    printf '  0) Back\n'
    read -r -p 'Choice: ' choice || return 0
    case "$choice" in
      1) engine=docker ;;
      2) engine=podman ;;
      0) return 0 ;;
      *) echo 'Invalid option'; continue ;;
    esac
    choose_security && review_browser_choice
  done
}

menu() {
  local choice
  [[ "${JELLY_UI_NESTED:-}" == 1 ]] || dev::brand compact
  while true; do
    dev::section 'BROWSER SETUP'
    detect_engines
    dev::section 'CHOOSE A PATH'
    dev::option 1 'Recommended configurations'
    dev::option 2 'Test a configuration'
    dev::option 3 'Advanced settings'
    dev::option 4 'Native Chromium'
    dev::option 5 'External Chromium'
    dev::option 6 'List browsers'
    dev::option 0 'Back'
    dev::prompt 'Choose an option: ' choice || return 0
    case "$choice" in
      1) choose_recommendation ;;
      2) choose_probe ;;
      3) advanced_menu ;;
      4)
        dev::section 'REVIEW  /  Native browser'
        dev::note 'Changes config/jelly.toml to native mode; MCP restart is required.'
        if dev::ask_yes 'Use native Chromium?'; then
          if native; then
            dev::success 'Native browser mode selected. Restart Jelly MCP to apply.'
            return 0
          else
            dev::warning 'Native browser configuration was not applied.'
          fi
        else
          dev::note 'Cancelled; no browser configuration changed.'
        fi ;;
      5)
        dev::section 'REVIEW  /  External Chromium'
        dev::prompt 'Existing CDP endpoint (IP:PORT or URL, blank to cancel): ' address || return 0
        [[ -n "$address" ]] || continue
        dev::item 'Remote CDP' "$address"
        dev::warning 'Anyone who can reach unauthenticated CDP may control that browser.'
        if dev::ask_yes 'Register this external endpoint?'; then
          if connect_external; then
            dev::success 'External browser registered. Restart Jelly MCP to reload its configuration.'
          else
            dev::warning 'External browser registration failed; check the address and connection.'
          fi
        else
          dev::note 'Cancelled; no remote browser registered.'
        fi ;;
      6)
        dev::prompt 'List engine [docker/podman]: ' engine || return 0
        list ;;
      0) return 0 ;;
      *) dev::warning 'Choose one of the numbered options.' ;;
    esac
  done
}

case "$action" in
  menu) menu;;
  detect) detect_engines;;
  recommend) recommend;;
  probe) probe;;
  add) add;;
  list) list;;
  stop) stop_one;;
  native) native;;
  connect) connect_external;;
  diagnose) diagnose;;
  help|-h|--help) usage;;
  *) usage >&2; exit 2;;
esac
