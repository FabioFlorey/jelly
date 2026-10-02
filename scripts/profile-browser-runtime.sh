#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=scripts/config.sh
source "$ROOT/scripts/config.sh"

BIN_DIR="$CONFIG_BUILD_ROOT/debug"
FIXTURE="file://$(pwd)/tests/fixtures/browser-perf.html"
RUNS="${JELLY_PROFILE_RUNS:-15}"
CONFIG_BACKUP="$(mktemp)"
cp "$CONFIG_FILE" "$CONFIG_BACKUP"

cleanup() {
  cp "$CONFIG_BACKUP" "$CONFIG_FILE"
  rm -f "$CONFIG_BACKUP"
  "${BIN_DIR}/agent-close-browser" >/dev/null 2>&1 || true
}
trap cleanup EXIT

jelly_config_set page runtime true
jelly_config_set diagnostics perf_log false

if ! [[ "${RUNS}" =~ ^[1-9][0-9]*$ ]]; then
  echo "JELLY_PROFILE_RUNS must be a positive integer" >&2
  exit 2
fi

cargo build --bins --quiet
"${BIN_DIR}/agent-close-browser" >/dev/null 2>&1 || true
"${BIN_DIR}/agent-open-browser" --headless "${FIXTURE}" >/dev/null

profile_case() {
  local case_name="$1"
  echo "--- ${case_name}: rebuild phases (ms) ---"
  "${BIN_DIR}/agent-evaluate-js" "window.__jellyBench.setCase('${case_name}')" >/dev/null
  "${BIN_DIR}/agent-snapshot-interactive" 1 >/dev/null
  "${BIN_DIR}/agent-evaluate-js" "globalThis.__jellyRuntimeV1.lastRebuildTimings"

  echo "--- ${case_name}: browser-side hot-path probes ---"
  "${BIN_DIR}/agent-evaluate-js" "(()=>{
    const rt=globalThis.__jellyRuntimeV1,items=rt.cache,runs=${RUNS};
    const bench=fn=>{
      const samples=[];
      for(let j=0;j<runs;j++){
        const t=performance.now();
        fn();
        samples.push(performance.now()-t)
      }
      samples.sort((a,b)=>a-b);
      const sum=samples.reduce((a,b)=>a+b,0);
      return {
        avg_ms:sum/samples.length,
        p50_ms:samples[Math.floor(samples.length*.5)],
        p90_ms:samples[Math.min(samples.length-1,Math.floor(samples.length*.9))]
      }
    };
    const fifty=rt.snapshot(50,0);
    const all=rt.snapshot(0,0);
    return {
      candidates:items.length,
      rects:bench(()=>{for(const x of items)x.e.getBoundingClientRect()}),
      styles:bench(()=>{for(const x of items){const s=getComputedStyle(x.e);void s.display;void s.visibility}}),
      describe_all:bench(()=>{for(const x of items)rt.describe(x)}),
      snapshot_50:bench(()=>rt.snapshot(50,0)),
      snapshot_all:bench(()=>rt.snapshot(0,0)),
      search_exact:bench(()=>rt.search('${case_name} exact target',10,0)),
      stringify_50:bench(()=>JSON.stringify(fifty)),
      stringify_all:bench(()=>JSON.stringify(all))
    }
  })()"
}

profile_case large
profile_case shadow

echo '--- bounded snapshot CDP comparison ---'
"${BIN_DIR}/agent-evaluate-js" "window.__jellyBench.setCase('large')" >/dev/null
"${BIN_DIR}/agent-snapshot-interactive" 1 >/dev/null
rm -f "$CONFIG_RUNTIME_ROOT/logs/perf.jsonl"
jelly_config_set diagnostics perf_log true
for _ in $(seq 1 "${RUNS}"); do
  "${BIN_DIR}/agent-snapshot-interactive" 50 >/dev/null
done
jq -s '{
  runtime_evaluate_avg_ms:
    ([.[] | select(.event=="cdp.call" and .detail=="Runtime.evaluate") | .duration_ms] |
      if length==0 then 0 else add/length end),
  browser_connect_avg_ms:
    ([.[] | select(.event=="browser.connect") | .duration_ms] |
      if length==0 then 0 else add/length end)
}' "$CONFIG_RUNTIME_ROOT/logs/perf.jsonl"
