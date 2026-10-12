#!/usr/bin/env bash
# Offline CLI contract tests. Never calls installed Jelly services or loads real .env.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CLI="$ROOT/scripts/dev.sh"
TMP="$(mktemp -d /tmp/jelly-dev-cli-contract.XXXXXXXX)"
trap 'rm -rf -- "$TMP"' EXIT
fail() { printf 'CLI contract failed: %s\n' "$*" >&2; exit 1; }

cd /tmp

# Bare dev.sh enters the REAL quickstart; without a terminal it must
# reject execution before checking services or changing user configuration.
if NO_COLOR=1 "$CLI" > "$TMP/quickstart-noninteractive" 2>&1; then
  fail 'bare dev.sh unexpectedly succeeded without a terminal'
fi
grep -Fq 'Quickstart needs an interactive terminal' "$TMP/quickstart-noninteractive" || fail 'bare CLI did not select quickstart'
if NO_COLOR=1 "$CLI" quickstart > "$TMP/quickstart-explicit-noninteractive" 2>&1; then
  fail 'explicit quickstart accepted a redirected session'
fi
"$CLI" quickstart --help > "$TMP/quickstart-help"
grep -Fq 'Guided real setup' "$TMP/quickstart-help" || fail 'quickstart help missing'
for removed in home --home guide --guide; do
  if "$CLI" "$removed" > "$TMP/legacy" 2>&1; then fail "retired $removed command still works"; fi
done
printf '0\n' | NO_COLOR=1 TERM=xterm "$CLI" browser menu > "$TMP/browser-menu"
grep -Fq 'Recommended configurations' "$TMP/browser-menu" || fail 'browser recommendation entry missing'
grep -Fq 'Native Chromium' "$TMP/browser-menu" || fail 'browser native entry missing'
if LC_ALL=C grep -q $'\033' "$TMP/browser-menu"; then fail 'browser NO_COLOR emitted ANSI'; fi
"$CLI" --help > "$TMP/help"
grep -Fq 'demo | --demo' "$TMP/help" || fail 'demo command flag missing from help'
grep -Fq 'quickstart' "$TMP/help" || fail 'real quickstart command missing from help'
if grep -Eq 'home|--guide' "$TMP/help"; then fail 'legacy home/guide commands still advertised'; fi
grep -Fq 'setup [--dry-run]' "$TMP/help" || fail 'setup missing from help'
grep -Fq 'test [--isolated|--ranking|--web-ui|--suite' "$TMP/help" || fail 'test modes missing from help'
if "$CLI" nonexistent > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'unknown command succeeded'; fi
grep -Fq 'unknown command' "$TMP/stderr" || fail 'unknown command error missing'
if "$CLI" test --invalid > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'invalid test selection succeeded'; fi
if "$CLI" setup --nonsense > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'invalid setup option succeeded'; fi
if "$CLI" setup --check > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'unsupported setup option succeeded'; fi
if "$CLI" clean --nonsense > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'invalid cleanup option succeeded'; fi
if "$CLI" benchmark surface > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'live benchmark ran without explicit --live'; fi
if "$CLI" benchmark unknown --live > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'unknown benchmark succeeded'; fi
if "$CLI" tools --generate --typo > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'unknown tool-index arguments succeeded'; fi


# Real quickstart is an interactive operation. An immediate cancellation from
# a mocked preflight must not install, start, register, or write anything.
if command -v python3 >/dev/null 2>&1; then
  mkdir -p "$TMP/quickstart-bin"
  for helper in cargo rustc systemd-run; do
    cat > "$TMP/quickstart-bin/$helper" <<'MOCK'
#!/usr/bin/env bash
exit 88
MOCK
    chmod +x "$TMP/quickstart-bin/$helper"
  done
  cat > "$TMP/quickstart-bin/systemctl" <<'MOCK'
#!/usr/bin/env bash
[[ "$*" == '--user is-system-running' ]] || {
  echo "$*" >> "$QUICKSTART_FORBIDDEN_CALLS"
  exit 88
}
MOCK
  chmod +x "$TMP/quickstart-bin/systemctl"
  export QUICKSTART_FORBIDDEN_CALLS="$TMP/quickstart-systemctl-calls"
  python3 - "$CLI" "$TMP/quickstart-bin:$PATH" <<'PY'
import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time

cli, cli_path = sys.argv[1:]
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 34, 120, 0, 0))
env = dict(os.environ, PATH=cli_path, TERM="xterm", NO_COLOR="1")
proc = subprocess.Popen([cli], stdin=slave, stdout=slave, stderr=slave, env=env)
os.close(slave)
os.write(master, b"0\n")
output = b""
deadline = time.monotonic() + 12
while time.monotonic() < deadline:
    readable, _, _ = select.select([master], [], [], 0.2)
    if readable:
        try:
            chunk = os.read(master, 32768)
        except OSError:
            break
        if not chunk:
            break
        output += chunk
    if proc.poll() is not None:
        break
if proc.poll() is None:
    proc.terminate()
proc.wait(timeout=3)
os.close(master)
if proc.returncode != 0:
    raise SystemExit("Bare quickstart failed its safe-cancel PTY test")
if b"Browser option [0-4]:" not in output or b"Quickstart cancelled." not in output:
    raise SystemExit("Bare quickstart failed to show live browser choices")
if b"WHAT WOULD YOU LIKE TO DO?" in output:
    raise SystemExit("Retired dashboard was displayed")
if b"Core prerequisites are satisfied." not in output:
    raise SystemExit("Quickstart did not complete the mock preflight")
PY
  [[ ! -e "$QUICKSTART_FORBIDDEN_CALLS" ]] || fail 'quickstart cancellation performed systemctl operations'
fi

# Simulation is presentation only, never a dry-run that calls real commands.
# Instrument forbidden actions so a regression fails instead of touching host state.
mkdir -p "$TMP/demo-bin"
for forbidden in docker podman systemctl systemd-run cargo rustc curl wget cloudflared \
                 xdg-open gio open sudo git touch mkdir rm mv cp chmod tee install \
                 truncate dd mktemp jq; do
  cat > "$TMP/demo-bin/$forbidden" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$(/usr/bin/basename "$0") $*" >> "$JELLY_DEMO_FORBIDDEN_LOG"
exit 89
MOCK
  chmod +x "$TMP/demo-bin/$forbidden"
done
export JELLY_DEMO_FORBIDDEN_LOG="$TMP/demo-forbidden-actions"
DEMO_PATH="$TMP/demo-bin:$PATH"
# Demo accepts actual inputs and changes sample state in memory, not host state.
NO_COLOR=1 TERM=xterm PATH="$DEMO_PATH" "$CLI" demo --tour > "$TMP/demo-tour"
NO_COLOR=1 TERM=xterm PATH="$DEMO_PATH" "$CLI" --demo --tour > "$TMP/demo-tour-flag"
cmp -s "$TMP/demo-tour" "$TMP/demo-tour-flag" || fail 'direct --demo differs from demo subcommand'
for section in 'JELLY  /  INTERACTIVE DEMO' 'Get started · MCP setup' \
               'MCP configuration' 'Configure browsers' 'Browser deployment' \
               'Tests and verification' 'Failure and recovery' 'Sample session'; do
  grep -Fq "$section" "$TMP/demo-tour" || fail "demo tour missing $section"
done
grep -Fq 'Apply these settings? [y/N]:' "$TMP/demo-tour" || fail 'setup confirmation missing'
grep -Fq 'Browser registered.' "$TMP/demo-tour" || fail 'example browser not registered'
if grep -Fq 'START  "I want my AI client' "$TMP/demo-tour"; then fail 'demo printed a business decision tree'; fi
if grep -Fq 'flowchart TD' "$TMP/demo-tour"; then fail 'demo printed Mermaid'; fi
if [[ -s "$JELLY_DEMO_FORBIDDEN_LOG" ]]; then fail "demo invoked an action: $(cat "$JELLY_DEMO_FORBIDDEN_LOG")"; fi
if LC_ALL=C grep -q $'\033' "$TMP/demo-tour"; then fail 'NO_COLOR demo emitted escape sequences'; fi
NO_COLOR='' TERM=xterm PATH="$DEMO_PATH" "$CLI" demo --tour > "$TMP/demo-no-color-empty"
if LC_ALL=C grep -q $'\033' "$TMP/demo-no-color-empty"; then fail 'empty NO_COLOR must suppress ANSI'; fi
NO_COLOR=1 COLUMNS=35 PATH="$DEMO_PATH" "$CLI" demo > "$TMP/demo-default"
grep -Fq 'Sample session' "$TMP/demo-default" || fail 'noninteractive demo not completed'
[[ "$(grep -Ec '[░▒]' "$TMP/demo-default")" -ge 16 ]] || fail 'demo must show original hero'
NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --hero > "$TMP/demo-hero"
[[ "$(grep -Ec '[░▒]' "$TMP/demo-hero")" -ge 16 ]] || fail 'full hero preview was truncated'
if LC_ALL=C grep -q $'\033' "$TMP/demo-hero"; then fail 'NO_COLOR hero emitted ANSI'; fi

# User-entered URL, Telegram choice and save-only choice affect review state.
printf '1\n1\n2\nhttps://jelly.example.com\n2\n1\ny\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/demo-hosting"
grep -Fq 'https://jelly.example.com' "$TMP/demo-hosting" || fail 'custom domain not reflected'
grep -Fq 'Telegram           yes' "$TMP/demo-hosting" || fail 'Telegram choice not reflected'
grep -Fq 'Save only' "$TMP/demo-hosting" || fail 'save-only mode not reflected'
grep -Fq 'MCP settings applied.' "$TMP/demo-hosting" || fail 'setup confirmation not applied'
grep -Fq 'Browser            none' "$TMP/demo-hosting" || fail 'setup changed browser state'

# Explicit rejection leaves the simulated state untouched.
printf '1\n2\n1\n1\nn\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/demo-cancel"
grep -Fq 'Cancelled.' "$TMP/demo-cancel" || fail 'setup rejection not displayed'
grep -Fq 'Client             unconfigured' "$TMP/demo-cancel" || fail 'cancelled setup modified client'

# A Podman sandbox-enabled, standard/loopback probe changes subsequent evidence.
printf '2\n2\n1\n1\n2\n1\n2\ny\n1\n1\n1\n2\ny\n0\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/demo-probe"
grep -Fq 'CDP PASS · example' "$TMP/demo-probe" || fail 'successful sample probe missing'
grep -Fq 'Demo probe         PASS' "$TMP/demo-probe" || fail 'probe result not retained'
grep -Fq 'Browser registered.' "$TMP/demo-probe" || fail 'sample browser registration missing'
grep -Fq 'Browser            podman · enabled · standard · isolated' "$TMP/demo-probe" || fail 'browser state not propagated'

# Failed combinations remain selectable and do not register a browser.
printf '2\n3\n1\n1\n1\n1\n2\ny\n0\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/demo-failed"
grep -Fq 'Browser could not start:' "$TMP/demo-failed" || fail 'failing combination claimed success'
! grep -Fq 'Browser registered.' "$TMP/demo-failed"
grep -Fq 'Browser            none' "$TMP/demo-failed" || fail 'failure changed browser'

# Integration demos require the user-selected sample state; errors are never
# invented for a missing browser or a stopped MCP service.
printf '5\n3\n1\n0\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/integration-no-browser"
grep -Fq 'NOT RUN · No browser has been selected.' "$TMP/integration-no-browser" || fail 'unconfigured browser was falsely failed'

printf '2\n4\ny\n0\n5\n3\n1\n0\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/integration-service-off"
grep -Fq 'NOT RUN · The sample MCP service is stopped.' "$TMP/integration-service-off" || fail 'stopped service incorrectly counted as browser failure'

printf '4\n3\ny\n0\n2\n4\ny\n0\n5\n3\n1\n0\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/integration-configured"
grep -Fq 'Browser integration PASS (example)' "$TMP/integration-configured" || fail 'ready sample browser did not pass'
grep -Fq 'Registered browser selected successfully' "$TMP/integration-configured" || fail 'MCP routing result missing'

printf '5\n3\n2\n0\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/integration-pass-example"
grep -Fq 'Browser integration PASS (example)' "$TMP/integration-pass-example" || fail 'explicit passing example unavailable'
printf '5\n3\n3\n0\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/integration-failure-example"
grep -Fq 'Browser integration FAIL (example)' "$TMP/integration-failure-example" || fail 'explicit failure example unavailable'
grep -Fq 'Namespace creation denied' "$TMP/integration-failure-example" || fail 'failure missing concrete explanation'
! grep -Fq 'Browser integration unavailable (example)' "$TMP/integration-failure-example"

# Recommendations adapt to Docker-only machines; no Podman candidate leaks in.
printf '6\n2\n2\n1\n1\n1\n2\ny\n0\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/demo-docker"
grep -Fq 'Docker rootful · sandbox disabled, restricted' "$TMP/demo-docker" || fail 'Docker-only recommendation missing'
! grep -Fq 'Podman rootless · sandbox enabled, standard' "$TMP/demo-docker"
grep -Fq 'Browser            docker · disabled · restricted · isolated' "$TMP/demo-docker" || fail 'Docker-only choice did not propagate'

# Bad input re-prompts rather than changing state or exiting.
printf 'invalid\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/demo-bad-choice"
grep -Fq 'Choose a number between 0 and 7.' "$TMP/demo-bad-choice" || fail 'invalid menu input not handled'

# A sample with neither engine still offers native and external Chromium.
printf '6\n4\n2\n4\ny\n0\n0\n' |
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --menu > "$TMP/demo-no-engine"
grep -Fq 'Sample environment: none' "$TMP/demo-no-engine" || fail 'sample environment did not change'
grep -Fq 'Native Chromium selected.' "$TMP/demo-no-engine" || fail 'native choice not selectable'
grep -Fq 'Browser            native' "$TMP/demo-no-engine" || fail 'native backend state missing'

for scene in setup browsers alternatives manage tests recovery; do
  NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --scenario "$scene" </dev/null > "$TMP/demo-$scene"
  grep -Fq 'JELLY  /  INTERACTIVE DEMO' "$TMP/demo-$scene" || fail "missing scenario $scene"
done
if NO_COLOR=1 PATH="$DEMO_PATH" "$CLI" demo --scenario unsupported > "$TMP/demo-invalid" 2>&1; then fail 'invalid demo scene accepted'; fi

# Business diagrams exist as a standalone Markdown artifact, not a CLI command.
diagram="$ROOT/docs/reference/diagrams/DEV.SH.md"
[[ "$(grep -c '^flowchart TD' "$diagram")" -eq 5 ]] || fail 'expected five Markdown Mermaid trees'
grep -Fq '# CLI Processes Diagrams' "$diagram" || fail 'document title missing'
[[ "$(grep -Fc '&uarr; Back to top' "$diagram")" -eq 5 ]] || fail 'chapter nav links missing'
[[ ! -e "$JELLY_DEMO_FORBIDDEN_LOG" ]] || fail 'demo or guide ran an operational command'

# A real PTY redraws with the original hero and pauses after an outcome.
# Piped interactions must remain free of clearing and extra input prompts.
if command -v python3 >/dev/null 2>&1; then
  python3 - "$CLI" "$DEMO_PATH" <<'PY'
import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time

cli, tool_path = sys.argv[1:]
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 42, 130, 0, 0))
env = dict(os.environ, PATH=tool_path, TERM="xterm", NO_COLOR="")
proc = subprocess.Popen([cli, "demo", "--menu"], stdin=slave, stdout=slave, stderr=slave, env=env)
os.close(slave)
# Home -> Tests -> Integration -> example PASS -> acknowledge -> back -> exit.
os.write(master, b"5\n3\n2\n\n0\n0\n")
captured = b""
until = time.monotonic() + 12
while time.monotonic() < until:
    readable, _, _ = select.select([master], [], [], 0.2)
    if readable:
        try:
            part = os.read(master, 32768)
        except OSError:
            break
        if not part:
            break
        captured += part
    if proc.poll() is not None:
        break
if proc.poll() is None:
    proc.terminate()
proc.wait(timeout=3)
os.close(master)
if proc.returncode != 0:
    raise SystemExit("Demo PTY did not exit cleanly")
if captured.count(b"\x1b[2J\x1b[H") < 4:
    raise SystemExit("Demo failed to clear and redraw the active screen")
if captured.decode("utf-8", "replace").count("░") < 16 * 4:
    raise SystemExit("Demo failed to redraw the original logo")
if b"Press Enter to continue" not in captured:
    raise SystemExit("Demo result was not held for confirmation")
if b"Browser integration PASS" not in captured:
    raise SystemExit("Demo test result was not visible")
# NO_COLOR suppresses formatting, but does not prevent intentional redraw.
if b"\x1b" in captured.replace(b"\x1b[2J\x1b[H", b""):
    raise SystemExit("Demo PTY emitted unexpected ANSI styling under NO_COLOR")
PY
fi

# Stub the service control executable so start/stop tests cannot touch real services.
mkdir -p "$TMP/bin"
cat > "$TMP/bin/systemctl" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$DEV_SYSTEMCTL_CALLS"
MOCK
chmod +x "$TMP/bin/systemctl"
export DEV_SYSTEMCTL_CALLS="$TMP/systemctl-calls"
PATH="$TMP/bin:$PATH" "$CLI" start
PATH="$TMP/bin:$PATH" "$CLI" stop
test "$(wc -l < "$DEV_SYSTEMCTL_CALLS")" -eq 4 || fail 'unexpected service call count'
grep -Fxq -- '--user start jelly-browser.service' "$DEV_SYSTEMCTL_CALLS" || fail 'browser startup missing'
grep -Fxq -- '--user start jelly-mcp.service' "$DEV_SYSTEMCTL_CALLS" || fail 'MCP startup missing'
grep -Fxq -- '--user stop jelly-mcp.service' "$DEV_SYSTEMCTL_CALLS" || fail 'MCP shutdown missing'
grep -Fxq -- '--user stop jelly-browser.service' "$DEV_SYSTEMCTL_CALLS" || fail 'browser shutdown missing'
PATH="$TMP/bin:$PATH" "$CLI" clean > "$TMP/preview" 2>&1
PATH="$TMP/bin:$PATH" "$CLI" clean --build > "$TMP/preview-build" 2>&1
PATH="$TMP/bin:$PATH" "$CLI" uninstall > "$TMP/uninstall-preview" 2>&1
[[ $(wc -l < "$DEV_SYSTEMCTL_CALLS") -eq 4 ]] || fail 'clean preview contacted systemd'
grep -Fq 'Preview only' "$TMP/preview" || fail 'cleanup preview warning missing'
grep -Fq -- 'scripts/commands/clean.sh --build' "$TMP/preview-build" || fail 'build cleanup scope missing'
grep -Fq 'uninstall --yes' "$TMP/uninstall-preview" || fail 'uninstall preview warning missing'
[[ $(wc -l < "$DEV_SYSTEMCTL_CALLS") -eq 4 ]] || fail 'uninstall preview contacted systemd'

# Explicit uninstall is exercised exclusively against a temporary unit directory
# with a mocked systemctl; the real user manager is never contacted.
export XDG_CONFIG_HOME="$TMP/mock-config"
mkdir -p "$XDG_CONFIG_HOME/systemd/user"
printf 'test unit\n' > "$XDG_CONFIG_HOME/systemd/user/jelly-mcp.service"
printf 'test unit\n' > "$XDG_CONFIG_HOME/systemd/user/jelly-cloudflared.service"
PATH="$TMP/bin:$PATH" "$CLI" uninstall --yes > "$TMP/uninstall-run"
[[ ! -e "$XDG_CONFIG_HOME/systemd/user/jelly-mcp.service" ]] || fail 'owned MCP mock unit remains'
[[ ! -e "$XDG_CONFIG_HOME/systemd/user/jelly-cloudflared.service" ]] || fail 'owned tunnel mock unit remains'
[[ $(wc -l < "$DEV_SYSTEMCTL_CALLS") -eq 7 ]] || fail 'unexpected uninstall systemctl call count'
grep -Fxq -- '--user daemon-reload' "$DEV_SYSTEMCTL_CALLS" || fail 'uninstall did not reload mocked user manager'

# No compiler or application binary may run merely because config was sourced.
cat > "$TMP/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
printf 'Unexpected Cargo invocation\n' >> "$DEV_CARGO_CALLS"
exit 88
MOCK
chmod +x "$TMP/bin/cargo"
export DEV_CARGO_CALLS="$TMP/cargo-calls"
PATH="$TMP/bin:$PATH" REPO_ROOT="$ROOT" bash -c 'source "$REPO_ROOT/scripts/lib/config.sh"; test -n "$CONFIG_BUILD_ROOT"'
[[ ! -e "$DEV_CARGO_CALLS" ]] || fail 'config sourcing compiled code'

# The CLI help must work when Cargo is absent from PATH (mock command would fail).
PATH="$TMP/bin:$PATH" "$CLI" help > /dev/null
[[ ! -e "$DEV_CARGO_CALLS" ]] || fail 'help invoked Cargo'
NO_COLOR=1 TERM=xterm PATH="$TMP/bin:$PATH" "$CLI" clean > "$TMP/nocolor" 2>&1
if LC_ALL=C grep -q $'\033' "$TMP/nocolor"; then fail 'NO_COLOR emitted ANSI escapes'; fi
if [[ -e "$DEV_CARGO_CALLS" ]]; then fail 'clean preview invoked Cargo'; fi

# Trusted CLI config must not contain unexpected process-spawning statements.
grep -Fq "DEV_BRAND_LOGO='config/brand/full-logo.txt'" "$ROOT/config/dev.config.sh" || fail 'canonical logo path missing'
test -s "$ROOT/config/brand/full-logo.txt" || fail 'logo asset missing'
test ! -e "$ROOT/quickstart.sh" || fail 'duplicate quickstart entrypoint remains'
test "$(find "$ROOT/scripts" -maxdepth 1 -type f -name '*.sh' | wc -l)" -eq 4 || fail 'unexpected top-level shell wrappers'
for removed in check-page-runtime.sh check-shadow-dom.sh check-test-suite.sh check-tool-index.sh build-tool-index.sh; do
  test ! -e "$ROOT/scripts/$removed" || fail "obsolete wrapper still exists: $removed"
done
printf 'PASS: dev.sh dispatcher, safe args, service controls mocked, non-destructive cleanup, source-only config and branding\n'
