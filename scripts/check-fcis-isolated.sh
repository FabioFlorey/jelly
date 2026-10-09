#!/usr/bin/env bash
# Disposable, Rust-driven integration. Never operates on installed Jelly services.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
for required in cargo rustc chromium tar mktemp sed; do
  command -v "$required" >/dev/null || { echo "missing required $required" >&2; exit 2; }
done
SANDBOX="$(mktemp -d /data/jelly-fcis-isolated-XXXXXXXX)"
cleanup() {
  if [[ "${JELLY_FCIS_KEEP_SANDBOX:-false}" == true ]]; then
    printf 'Isolated sandbox retained: %s\n' "$SANDBOX"
    return
  fi
  [[ "$SANDBOX" == /data/jelly-fcis-isolated-* && "$SANDBOX" != /data/jelly-fcis-isolated- ]] || return 2
  rm -rf -- "$SANDBOX"
}
trap cleanup EXIT
mkdir -p "$SANDBOX/repo" "$SANDBOX/runtime" "$SANDBOX/build"
# The working tree, not a Git checkout: include uncommitted files, exclude secrets.
tar -C "$ROOT" --exclude='./.git' --exclude='./.env' --exclude='./EVIDENCE.md' -cf - . \
  | tar -C "$SANDBOX/repo" -xf -
CONFIG="$SANDBOX/repo/config/jelly.toml"
CARGO_CONFIG="$SANDBOX/repo/config/cargo.toml"
grep -Fxq 'runtime_root = "/data/jelly-runtime"' "$CONFIG" || { echo 'unexpected runtime config' >&2; exit 2; }
grep -Fxq 'target-dir = "/data/.jelly-build"' "$CARGO_CONFIG" || { echo 'unexpected Cargo config' >&2; exit 2; }
[[ ! -e "$SANDBOX/repo/.env" ]] || { echo 'refusing sandbox containing .env' >&2; exit 2; }
sed -i "s|^runtime_root = \"/data/jelly-runtime\"$|runtime_root = \"$SANDBOX/runtime\"|" "$CONFIG"
sed -i "s|^target-dir = \"/data/.jelly-build\"$|target-dir = \"$SANDBOX/build\"|" "$CARGO_CONFIG"
printf 'Isolated FC/IS sandbox: %s\n' "$SANDBOX"
cd "$SANDBOX/repo"
export CARGO_TARGET_DIR="$SANDBOX/build"
cargo build --locked --quiet \
  --bin jelly-maint --bin jelly-fcis-probe --bin jelly-fixture-server \
  --bin jelly-mcp --bin agent-find-interactive --bin agent-snapshot-interactive \
  --bin agent-evaluate-js --bin agent-get-element --bin agent-tabs \
  --bin agent-click --bin agent-downloads --bin agent-download \
  --bin agent-read-page --bin agent-navigate --bin agent-close-tab \
  --bin agent-switch-tab --bin agent-call-routine
cargo build --locked --quiet --manifest-path tests/suite/support/agent-api-probe/Cargo.toml
"$SANDBOX/build/debug/jelly-fcis-probe" "$SANDBOX"
"$SANDBOX/build/debug/jelly-maint" test ranking
printf 'PASS: Rust-only isolated FC/IS integration and Chromium ranking\n'
