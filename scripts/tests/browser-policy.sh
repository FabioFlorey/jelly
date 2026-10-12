#!/usr/bin/env bash
# Offline container policy contract test: uses mock engines, never starts real containers.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP="$(mktemp -d /tmp/jelly-browser-policy.XXXXXXXX)"
trap 'rm -rf -- "$TMP"' EXIT
mkdir -p "$TMP/bin"
cat > "$TMP/bin/docker" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$BROWSER_ENGINE_CALLS"
case "$1 $2" in
  "info "*)
    if [[ "${0##*/}" == podman ]]; then
      printf '%s\n' "${BROWSER_MOCK_PODMAN_ROOTLESS:-false}"
    else
      if [[ "${BROWSER_MOCK_DOCKER_ROOTLESS:-}" == true ]]; then
        printf '%s\n' '["name=rootless","name=seccomp"]'
      else
        printf '%s\n' '["name=seccomp,profile=builtin"]'
      fi
    fi
    exit 0 ;;
  "--version "*) echo "${BROWSER_MOCK_VERSION:-mock-engine-1}"; exit 0 ;;
  "image inspect") echo 'sha256:mock-probe-image'; exit 0 ;;
  "port "*) echo '127.0.0.1:19999'; exit 0 ;;
  "network create"|"build -q") exit 0 ;;
  "network inspect") exit 1 ;;
  "container inspect") exit 1 ;;
  "run -d")
    if [[ "${BROWSER_FAIL_RUN:-}" == 1 ]]; then
      echo 'mock: sandbox not supported by this engine' >&2
      exit 125
    fi
    printf 'fake-container-id\n'; exit 0 ;;
  "inspect -f")
    case "$3" in
      *Networks*) printf '172.19.0.2\n';;
      *jelly.browser*) printf '1\n';;
      *jelly.security.sandbox*) printf 'enabled\n';;
      *jelly.security.engine*) printf 'standard\n';;
      *jelly.connection.exposure*) printf 'published\n';;
      *Running*) printf 'true\n';;
    esac
    exit 0 ;;
  *) exit 0 ;;
esac
MOCK
cat > "$TMP/bin/curl" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$BROWSER_CURL_CALLS"
if [[ "$*" == */json/version* ]]; then
  printf '%s\n' '{"Browser":"Chrome/154.0.8037.92","webSocketDebuggerUrl":"ws://127.0.0.1:19999/devtools/browser/mock"}'
fi
exit 0
MOCK
cat > "$TMP/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$BROWSER_REGISTRY_CALLS"
exit 0
MOCK
chmod +x "$TMP/bin/"*
cp "$TMP/bin/docker" "$TMP/bin/podman"
export BROWSER_ENGINE_CALLS="$TMP/engine" BROWSER_CURL_CALLS="$TMP/curl" BROWSER_REGISTRY_CALLS="$TMP/registrations"
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add --engine docker --profile isolated --sandbox enabled --engine-security standard >"$TMP/output"
grep -Fq -- '--env JELLY_CHROMIUM_SANDBOX=enabled' "$TMP/engine"
! grep -q -- 'no-new-privileges' "$TMP/engine"
grep -Fq -- '--connection subnet' "$TMP/registrations"
grep -Fq 'sandbox state is NOT attested' "$TMP/output"

: >"$TMP/engine"; : >"$TMP/registrations"
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add --engine docker --profile isolated --sandbox disabled --engine-security restricted --publish 0.0.0.0:9223 >"$TMP/output"
grep -Fq -- '--security-opt no-new-privileges --cap-drop ALL' "$TMP/engine"
grep -Fq -- '-p 0.0.0.0:9223:9222' "$TMP/engine"
grep -Fq -- '--connection address' "$TMP/registrations"
grep -Fq -- 'http://127.0.0.1:9223' "$TMP/registrations"
grep -Fq 'SECURITY: publishing Chromium CDP' "$TMP/output"

: >"$TMP/engine"; : >"$TMP/registrations"
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add --engine docker --profile ephemeral --build-network host --publish 127.0.0.1:9227 >"$TMP/output"
grep -Fq -- 'build -q --network host' "$TMP/engine"
grep -Fq -- '--tmpfs /data/profile:rw,nosuid,nodev,size=512m,mode=1777' "$TMP/engine"
grep -Fq -- '--tmpfs /browser-downloads:rw,nosuid,nodev,size=512m,mode=1777' "$TMP/engine"
grep -Fq 'operator-selected host networking applies only to image build' "$TMP/output"

: >"$TMP/engine"
env -u DBUS_SESSION_BUS_ADDRESS PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add --engine podman --profile ephemeral --publish 127.0.0.1:9228 >"$TMP/output"
grep -Fq -- 'network create --driver bridge --disable-dns jelly-browsers' "$TMP/engine"
grep -Fq 'Podman bridge DNS plugin disabled' "$TMP/output"
: >"$TMP/engine"
env -u DBUS_SESSION_BUS_ADDRESS PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add --engine podman --profile ephemeral --publish 127.0.0.1:9228 --bridge-dns enabled >"$TMP/output"
! grep -Fq -- '--disable-dns' "$TMP/engine"

# Orthogonality contract: every engine/sandbox/restriction tuple must forward
# exactly the requested options. Mock mode checks intent, not kernel enforcement.
for choice_engine in docker podman; do
  for choice_sandbox in enabled disabled; do
    for choice_security in restricted standard; do
      : >"$TMP/engine"; : >"$TMP/registrations"; : >"$TMP/curl"
      PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add \
        --engine "$choice_engine" --profile isolated \
        --sandbox "$choice_sandbox" --engine-security "$choice_security" \
        --publish 127.0.0.1:19300 >"$TMP/output"
      grep -Fq -- "JELLY_CHROMIUM_SANDBOX=$choice_sandbox" "$TMP/engine"
      grep -Fq -- "jelly.security.sandbox=$choice_sandbox" "$TMP/engine"
      grep -Fq -- "jelly.security.engine=$choice_security" "$TMP/engine"
      grep -Fq -- '-p 127.0.0.1:19300:9222' "$TMP/engine"
      grep -Fq -- '--connection address' "$TMP/registrations"
      # UID is fixed by the image; no CLI --user override must appear.
      ! grep -Eq -- '(^|[[:space:]])--user([=[:space:]]|$)' "$TMP/engine"
      if [[ "$choice_security" == restricted ]]; then
        grep -Fq -- '--security-opt no-new-privileges --cap-drop ALL' "$TMP/engine"
      else
        ! grep -Fq -- '--security-opt no-new-privileges' "$TMP/engine"
        ! grep -Fq -- '--cap-drop ALL' "$TMP/engine"
      fi
      # Sandbox and engine restrictions must not silently constrain one another.
      echo "PASS: forwarded $choice_engine sandbox=$choice_sandbox security=$choice_security"
    done
  done
done

: >"$TMP/engine"
if PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add --bridge-dns invalid >"$TMP/out" 2>&1; then
  echo 'Invalid bridge DNS was accepted' >&2; exit 1
fi
[[ ! -s "$TMP/engine" ]] || { echo 'Engine contacted for invalid bridge DNS' >&2; exit 1; }
if PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add --build-network invalid >"$TMP/out" 2>&1; then
  echo 'Invalid build network was accepted' >&2; exit 1
fi
[[ ! -s "$TMP/engine" ]] || { echo 'Engine contacted for invalid build network' >&2; exit 1; }
if PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add --publish 999.0.0.1:9222 >"$TMP/out" 2>&1; then
  echo 'Invalid IP was accepted' >&2; exit 1
fi
[[ ! -s "$TMP/engine" ]] || { echo 'Engine contacted for invalid IP' >&2; exit 1; }
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser diagnose --engine docker --container jelly-chromium-1 >"$TMP/diagnosis"
grep -Fq 'Requested sandbox: enabled' "$TMP/diagnosis"
grep -Fq 'Sandbox effectively active: UNVERIFIED' "$TMP/diagnosis"
if BROWSER_FAIL_RUN=1 PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser add --engine docker --profile isolated --sandbox enabled --engine-security restricted >"$TMP/failed-run" 2>&1; then
  echo 'Engine failure was treated as success' >&2; exit 1
fi
grep -Fq 'did not modify your security choices' "$TMP/failed-run"
grep -Fq 'Suggestions: check unprivileged user namespace' "$TMP/failed-run"

# Read-only discovery never builds or runs containers, and never registers one.
export XDG_CACHE_HOME="$TMP/probe-cache"
: >"$TMP/engine"; : >"$TMP/registrations"
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser detect >"$TMP/detect"
grep -Eq 'docker[[:space:]]+Available · rootful' "$TMP/detect"
grep -Eq 'podman[[:space:]]+Available · rootful' "$TMP/detect"
[[ ! -s "$TMP/registrations" ]] || { echo 'Discovery registered a browser' >&2; exit 1; }
! grep -Eq '^(build|run|network create)' "$TMP/engine"

: >"$TMP/engine"
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser recommend >"$TMP/recommend-before"
grep -Fq 'Reference PASS, not yet verified' "$TMP/recommend-before"
! grep -Fq 'LOCAL CDP PASS' "$TMP/recommend-before"
! grep -Eq '^(build|run|network create)' "$TMP/engine"

export BROWSER_MOCK_DOCKER_ROOTLESS=true
# A probe verifies real-CDP-shaped response, persists a local result, and never
# touches the browser registry. Engine calls and cleanup are asserted in mock.
: >"$TMP/engine"; : >"$TMP/registrations"
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser probe \
  --engine docker --sandbox enabled --engine-security standard \
  --profile isolated --publish 127.0.0.1:19310 >"$TMP/probe-pass"
grep -Fq 'LOCAL CDP PASS' "$TMP/probe-pass"
grep -Fq 'Effective sandbox: UNVERIFIED' "$TMP/probe-pass"
grep -Fq -- 'run -d --name jelly-probe-' "$TMP/engine"
grep -Fq -- '-p 127.0.0.1::9222' "$TMP/engine"
grep -Fq -- 'volume create jelly-probe-' "$TMP/engine"
grep -Fq -- 'rm -f jelly-probe-' "$TMP/engine"
grep -Fq -- 'network rm jelly-probe-' "$TMP/engine"
[[ ! -s "$TMP/registrations" ]] || { echo 'Probe registered a browser' >&2; exit 1; }
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser recommend >"$TMP/recommend-after"
grep -Fq 'LOCAL CDP PASS' "$TMP/recommend-after"

# Failure is stored as FAIL; it never changes the requested configuration.
if BROWSER_FAIL_RUN=1 PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser probe \
  --engine docker --sandbox enabled --engine-security restricted \
  --profile isolated --publish 127.0.0.1:19310 >"$TMP/probe-fail" 2>&1; then
  echo 'An engine failure was presented as probe PASS' >&2; exit 1
fi
grep -Fq 'Container launch rejected; no alternative settings attempted.' "$TMP/probe-fail"
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser recommend >"$TMP/recommend-failed"
grep -Fq 'Local CDP FAIL (still selectable)' "$TMP/recommend-failed"

# Changing the engine version invalidates BOTH old PASS and FAIL automatically.
BROWSER_MOCK_VERSION=mock-engine-2 PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser recommend >"$TMP/recommend-invalidated"
! grep -Fq 'LOCAL CDP PASS' "$TMP/recommend-invalidated"
grep -Fq 'Reference PASS, not yet verified' "$TMP/recommend-invalidated"

# Explicit rootless Podman network choice is not silently changed.
if BROWSER_MOCK_PODMAN_ROOTLESS=true PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser probe \
    --engine podman --sandbox disabled --engine-security restricted --profile ephemeral \
    >"$TMP/podman-missing-port" 2>&1; then
  echo 'Rootless Podman without an explicit publish was silently changed' >&2
  tail -n 15 "$TMP/podman-missing-port" >&2
  exit 1
fi
grep -Fq 'needs an explicitly selected --publish' "$TMP/podman-missing-port"

# Loopback success cannot be cached as validation of a LAN bind choice.
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser probe \
  --engine docker --sandbox disabled --engine-security standard \
  --profile isolated --publish 0.0.0.0:19320 >"$TMP/probe-lan"
grep -Fq 'LOOPBACK CDP PASS' "$TMP/probe-lan"
grep -Fq 'Requested LAN publication 0.0.0.0:19320: UNVERIFIED' "$TMP/probe-lan"
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser recommend >"$TMP/recommend-no-lan-proof"
grep -A 1 'sandbox=disabled  security=standard' "$TMP/recommend-no-lan-proof" | grep -Fq 'UNVERIFIED'

# Shared-mode probe does not claim to verify the host bind mount or cache an
# equivalent fully tested shared-volume configuration.
before="$(find "$XDG_CACHE_HOME" -type f | wc -l)"
PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser probe \
  --engine docker --sandbox disabled --engine-security standard \
  --profile shared --publish 127.0.0.1:19310 >"$TMP/probe-shared"
grep -Fq 'shared-download host bind mount is NOT tested' "$TMP/probe-shared"
after="$(find "$XDG_CACHE_HOME" -type f | wc -l)"
[[ "$after" -eq "$before" ]] || { echo 'Shared download probe cached unsupported evidence' >&2; exit 1; }

# Interactive recommendations require explicit confirmation before registration.
: >"$TMP/registrations"
printf '1\n1\n\n127.0.0.1:19310\n\nn\n0\n' |
  PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser menu >"$TMP/menu-cancel"
grep -Fq 'Rootless CDP tests use explicit 127.0.0.1 publication' "$TMP/menu-cancel"
grep -Fq 'Cancelled; no browser created.' "$TMP/menu-cancel"
[[ ! -s "$TMP/registrations" ]] || { echo 'Cancelled menu selection registered a browser' >&2; exit 1; }


# Browser menu modifications require a preview + explicit consent.
: > "$TMP/engine"; : > "$TMP/registrations"
config_before="$(sha256sum "$ROOT/config/jelly.toml" | cut -d' ' -f1)"
printf '4\nn\n0\n' | PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser menu >"$TMP/native-cancel"
grep -Fq 'Cancelled; no browser configuration changed.' "$TMP/native-cancel"
[[ "$(sha256sum "$ROOT/config/jelly.toml" | cut -d' ' -f1)" == "$config_before" ]] || {
  echo 'Native cancellation changed jelly.toml' >&2; exit 1;
}

: > "$TMP/registrations"
printf '5\nexample.invalid:9222\nn\n0\n' |
  PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser menu >"$TMP/external-cancel"
grep -Fq 'Cancelled; no remote browser registered.' "$TMP/external-cancel"
[[ ! -s "$TMP/registrations" ]] || { echo 'External cancellation registered a browser' >&2; exit 1; }

: > "$TMP/engine"; : > "$TMP/registrations"
printf '3\n1\n\n\n\n\n\nn\n0\n0\n' |
  PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser menu >"$TMP/advanced-cancel"
grep -Fq 'REVIEW  /  Browser deployment' "$TMP/advanced-cancel"
grep -Fq 'Cancelled; no browser created.' "$TMP/advanced-cancel"
! grep -Eq '^run -d |^build -q' "$TMP/engine"
[[ ! -s "$TMP/registrations" ]] || { echo 'Advanced cancellation registered a browser' >&2; exit 1; }

# An engine failure must never be followed by a success result.
printf '3\n1\n\n\n\n\n\ny\n0\n0\n' |
  BROWSER_FAIL_RUN=1 PATH="$TMP/bin:$PATH" "$ROOT/scripts/dev.sh" browser menu >"$TMP/advanced-failed" 2>&1 || :
grep -Fq 'Browser creation or registration failed' "$TMP/advanced-failed"
! grep -Fq 'Browser created and registered' "$TMP/advanced-failed"

echo 'PASS: requested policy preserved, safe UI confirmations, local probe/cache and independent choices'
