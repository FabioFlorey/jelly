#!/usr/bin/env bash
# Offline CLI contract tests. Never calls installed Jelly services or loads real .env.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CLI="$ROOT/scripts/dev.sh"
TMP="$(mktemp -d /tmp/jelly-dev-cli-contract.XXXXXXXX)"
trap 'rm -rf -- "$TMP"' EXIT
fail() { printf 'CLI contract failed: %s\n' "$*" >&2; exit 1; }

cd /tmp
"$CLI" --help > "$TMP/help"
grep -Fq 'setup [--dry-run]' "$TMP/help" || fail 'setup missing from help'
grep -Fq 'test [--isolated|--ranking|--suite' "$TMP/help" || fail 'test modes missing from help'
if "$CLI" nonexistent > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'unknown command succeeded'; fi
grep -Fq 'unknown command' "$TMP/stderr" || fail 'unknown command error missing'
if "$CLI" test --invalid > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'invalid test selection succeeded'; fi
if "$CLI" setup --nonsense > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'invalid setup option succeeded'; fi
if "$CLI" clean --nonsense > "$TMP/stdout" 2> "$TMP/stderr"; then fail 'invalid cleanup option succeeded'; fi

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
[[ $(wc -l < "$DEV_SYSTEMCTL_CALLS") -eq 4 ]] || fail 'clean preview contacted systemd'
grep -Fq 'Preview only' "$TMP/preview" || fail 'cleanup preview warning missing'
grep -Fq -- 'scripts/clean-runtime.sh --build' "$TMP/preview-build" || fail 'build cleanup scope missing'

# No compiler or application binary may run merely because config was sourced.
cat > "$TMP/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
printf 'Unexpected Cargo invocation\n' >> "$DEV_CARGO_CALLS"
exit 88
MOCK
chmod +x "$TMP/bin/cargo"
export DEV_CARGO_CALLS="$TMP/cargo-calls"
PATH="$TMP/bin:$PATH" REPO_ROOT="$ROOT" bash -c 'source "$REPO_ROOT/scripts/config.sh"; test -n "$CONFIG_BUILD_ROOT"'
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
printf 'PASS: dev.sh dispatcher, safe args, service controls mocked, non-destructive cleanup, source-only config and branding\n'
