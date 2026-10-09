#!/usr/bin/env bash
# Run the FC/IS integration suite from a sanitized copy of the current worktree.
# Never invoke Jelly launchers, systemd units, or the production runtime root.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
[[ "$ROOT" == "/data/github/jelly" || -f "$ROOT/Cargo.toml" ]] || {
  echo "Jelly repository not found" >&2
  exit 2
}
command -v cargo >/dev/null
command -v chromium >/dev/null
command -v tar >/dev/null
command -v python3 >/dev/null

SANDBOX="$(mktemp -d /data/jelly-fcis-isolated-XXXXXXXX)"
keep="${JELLY_FCIS_KEEP_SANDBOX:-false}"
cleanup() {
  if [[ "$keep" == true ]]; then
    printf 'Isolated sandbox retained at %s\n' "$SANDBOX"
    return
  fi
  # This invocation created SANDBOX, so only that exact, validated path can
  # be removed. No configured Jelly runtime, browser, or service is touched.
  python3 - "$SANDBOX" <<'PY'
from pathlib import Path
import shutil, sys
sandbox = Path(sys.argv[1])
assert sandbox.parent == Path('/data') and sandbox.name.startswith('jelly-fcis-isolated-')
shutil.rmtree(sandbox)
PY
}
trap cleanup EXIT

mkdir -p "$SANDBOX/repo" "$SANDBOX/runtime" "$SANDBOX/build"
# Copy the working tree including new/untracked core & shell code, not user
# secrets or Git internals. No source configuration is modified in place.
tar -C "$ROOT" --exclude='./.git' --exclude='./.env' --exclude='./EVIDENCE.md' -cf - . \
  | tar -C "$SANDBOX/repo" -xf -
python3 - "$SANDBOX" <<'PY'
from pathlib import Path
import sys
sandbox = Path(sys.argv[1]); repo = sandbox / 'repo'
config = repo / 'config/jelly.toml'
s = config.read_text()
old = 'runtime_root = "/data/jelly-runtime"'
if old not in s:
    raise SystemExit('unexpected runtime path in Jelly config; refusing unsafe test')
config.write_text(s.replace(old, f'runtime_root = "{sandbox}/runtime"'))
cargo = repo / 'config/cargo.toml'
s = cargo.read_text()
old = 'target-dir = "/data/.jelly-build"'
if old not in s:
    raise SystemExit('unexpected Cargo path in config; refusing unsafe test')
cargo.write_text(s.replace(old, f'target-dir = "{sandbox}/build"'))
if (repo / '.env').exists():
    raise SystemExit('refusing sandbox containing .env')
PY

printf 'FCIS sandbox: %s\n' "$SANDBOX"
cd "$SANDBOX/repo"
export CARGO_TARGET_DIR="$SANDBOX/build"
cargo build --locked --quiet \
  --bin jelly-mcp \
  --bin agent-find-interactive \
  --bin agent-snapshot-interactive \
  --bin agent-evaluate-js \
  --bin agent-get-element \
  --bin agent-tabs \
  --bin agent-click \
  --bin agent-downloads \
  --bin agent-download \
  --bin agent-read-page \
  --bin agent-navigate \
  --bin agent-close-tab \
  --bin agent-switch-tab \
  --bin agent-call-routine
cargo build --locked --quiet --manifest-path tests/suite/support/agent-api-probe/Cargo.toml
python3 tests/integration/fcis_isolated.py --sandbox "$SANDBOX"
if command -v node >/dev/null; then
  node tests/unit/ranking.test.cjs --browser
fi
printf 'FCIS isolated integration complete.\n'
