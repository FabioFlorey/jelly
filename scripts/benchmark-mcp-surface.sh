#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

probe_manifest="tests/suite/support/agent-api-probe/Cargo.toml"
probe_bin="/data/.jelly-build/debug/jelly-agent-api-probe"
fixture="file:///data/jelly/tests/fixtures/browser-perf.html"

CARGO_TARGET_DIR=/data/.jelly-build cargo build --quiet --locked --manifest-path "$probe_manifest"

echo "== Static MCP surface =="
"$probe_bin" surface-report

echo
echo "== Warm local runtime =="
/data/.jelly-build/debug/agent-close-browser >/dev/null 2>&1 || true
trap '/data/.jelly-build/debug/agent-close-browser >/dev/null 2>&1 || true' EXIT
/data/.jelly-build/debug/agent-open-browser --headless "$fixture" >/dev/null
"$probe_bin" latency-report
