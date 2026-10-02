#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=scripts/config.sh
source "$ROOT/scripts/config.sh"

probe_manifest="tests/suite/support/agent-api-probe/Cargo.toml"
probe_bin="$CONFIG_BUILD_ROOT/debug/jelly-agent-api-probe"
fixture="file://$ROOT/tests/fixtures/browser-perf.html"

CARGO_TARGET_DIR="$CONFIG_BUILD_ROOT" cargo build --quiet --locked --manifest-path "$probe_manifest"

echo "== Static MCP surface =="
"$probe_bin" surface-report

echo
echo "== Warm local runtime =="
"$CONFIG_BUILD_ROOT/debug/agent-close-browser" >/dev/null 2>&1 || true
trap '"$CONFIG_BUILD_ROOT/debug/agent-close-browser" >/dev/null 2>&1 || true' EXIT
"$CONFIG_BUILD_ROOT/debug/agent-open-browser" --headless "$fixture" >/dev/null
"$probe_bin" latency-report
