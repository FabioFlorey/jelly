#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

BIN_DIR="${JELLY_BIN_DIR:-/data/.jelly-build/debug}"
RUNS="${JELLY_BENCH_RUNS:-7}"
MCP_PORT="${JELLY_BENCH_MCP_PORT:-18787}"
FIXTURE="$(pwd)/tests/fixtures/browser-perf.html"
PERF_LOG="/data/jelly-runtime/logs/perf.jsonl"
RESULTS="${JELLY_BENCH_RESULTS:-/tmp/jelly-browser-perf-$(date +%Y%m%d-%H%M%S).tsv}"
META="${RESULTS%.tsv}.meta"
SERVER_PID=""

cleanup() {
  if [[ -n "${SERVER_PID}" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" >/dev/null 2>&1 || true
  fi
  "${BIN_DIR}/agent-close-browser" >/dev/null 2>&1 || true
}
trap cleanup EXIT

if ! [[ "${RUNS}" =~ ^[1-9][0-9]*$ ]]; then
  echo "JELLY_BENCH_RUNS must be a positive integer" >&2
  exit 2
fi

for dependency in jq curl python3; do
  command -v "${dependency}" >/dev/null 2>&1 || {
    echo "missing benchmark dependency: ${dependency}" >&2
    exit 2
  }
done

cargo build --bins --quiet

open_benchmark_browser() {
  local attempt
  "${BIN_DIR}/agent-close-browser" >/dev/null 2>&1 || true
  for attempt in 1 2 3; do
    sleep 0.25
    if "${BIN_DIR}/agent-open-browser" --headless "file://${FIXTURE}" >/dev/null 2>&1 \
      && "${BIN_DIR}/agent-evaluate-js" "document.title" >/dev/null 2>&1; then
      return 0
    fi
    "${BIN_DIR}/agent-close-browser" >/dev/null 2>&1 || true
  done
  echo "failed to start a ready benchmark browser after 3 attempts" >&2
  return 1
}

open_benchmark_browser

printf 'scope\tmode\tcase\toperation\trun\tduration_ms\tbytes\tconnect_ms\tcdp_ms\n' > "${RESULTS}"
{
  printf 'timestamp_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf 'git_head=%s\n' "$(git rev-parse HEAD 2>/dev/null || printf unknown)"
  printf 'git_dirty=%s\n' "$(git diff --quiet && git diff --cached --quiet && printf false || printf true)"
  printf 'runs=%s\n' "${RUNS}"
  printf 'rustc=%s\n' "$(rustc --version 2>/dev/null || printf unknown)"
  printf 'chromium=%s\n' "$(/usr/bin/chromium --version 2>/dev/null || printf unknown)"
  printf 'kernel=%s\n' "$(uname -sr 2>/dev/null || printf unknown)"
} > "${META}"

now_ns() {
  date +%s%N
}

perf_sum() {
  local event="$1" detail="${2:-}"
  [[ -s "${PERF_LOG}" ]] || { printf '0'; return; }
  jq -s -r --arg event "${event}" --arg detail "${detail}" '
    [ .[] | select(.event == $event and ($detail == "" or (.detail // "") == $detail)) | .duration_ms ]
    | if length == 0 then 0 else add end
  ' "${PERF_LOG}"
}

record_cli() {
  local mode="$1" case_name="$2" operation="$3" run="$4"
  shift 4
  local out start end duration bytes connect_ms cdp_ms
  out="$(mktemp)"
  rm -f "${PERF_LOG}"
  start="$(now_ns)"
  JELLY_PAGE_RUNTIME="${mode}" JELLY_PERF_LOG=1 "$@" >"${out}"
  end="$(now_ns)"
  duration="$(( (end - start) / 1000000 ))"
  bytes="$(wc -c < "${out}" | tr -d ' ')"
  connect_ms="$(perf_sum browser.connect)"
  cdp_ms="$(perf_sum cdp.call Runtime.evaluate)"
  printf 'cli\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${mode}" "${case_name}" "${operation}" "${run}" "${duration}" "${bytes}" "${connect_ms}" "${cdp_ms}" >> "${RESULTS}"
  rm -f "${out}"
}

set_case() {
  local case_name="$1"
  "${BIN_DIR}/agent-evaluate-js" "window.__jellyBench.setCase('${case_name}')" >/dev/null
}

reset_case() {
  local case_name="$1"
  "${BIN_DIR}/agent-navigate" "file://${FIXTURE}" >/dev/null
  set_case "${case_name}"
}

lookup_query() {
  case "$1" in
    small) printf '%s' 'small action 18' ;;
    medium) printf '%s' 'medium action 297' ;;
    large) printf '%s' 'large exact target' ;;
    mutation) printf '%s' 'mutation action 498' ;;
    shadow) printf '%s' 'nested shadow target' ;;
    *) return 1 ;;
  esac
}

benchmark_dom_mode() {
  local mode="$1" case_name query run
  for case_name in small medium large mutation shadow; do
    # Reload the fixture for every mode/case so "cold" means a fresh document,
    # not merely a dirty runtime carried over from the previous benchmark case.
    reset_case "${case_name}"
    query="$(lookup_query "${case_name}")"

    # First call includes runtime bootstrap/rebuild cost on a fresh document.
    record_cli "${mode}" "${case_name}" snapshot-cold 0       "${BIN_DIR}/agent-snapshot-interactive"

    for ((run = 1; run <= RUNS; run++)); do
      record_cli "${mode}" "${case_name}" snapshot-warm "${run}"         "${BIN_DIR}/agent-snapshot-interactive"
      record_cli "${mode}" "${case_name}" semantic-search "${run}"         "${BIN_DIR}/agent-find-interactive" "${query}" 10
      if [[ "${case_name}" == "large" ]]; then
        record_cli "${mode}" "${case_name}" snapshot-50 "${run}"           "${BIN_DIR}/agent-snapshot-interactive" 50
        record_cli "${mode}" "${case_name}" snapshot-50-offset-50 "${run}"           "${BIN_DIR}/agent-snapshot-interactive" 50 50
        record_cli "${mode}" "${case_name}" target-interactive "${run}"           "${BIN_DIR}/agent-element-info" "text:${query}"
        record_cli "${mode}" "${case_name}" target-generic "${run}"           "${BIN_DIR}/agent-element-info" "text:non interactive content 4999"
      fi
    done

    if [[ "${case_name}" == "mutation" ]]; then
      "${BIN_DIR}/agent-evaluate-js" "window.__jellyBench.mutateBurst(250)" >/dev/null
      record_cli "${mode}" "${case_name}" snapshot-after-mutation 0         "${BIN_DIR}/agent-snapshot-interactive"
    fi
  done
}

benchmark_dom_mode 0
benchmark_dom_mode 1

if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi

if [[ -n "${JELLY_MCP_TOKEN:-}" && -n "${JELLY_BOOTSTRAP_SECRET:-}" ]]; then
  mcp_call() {
    local id="$1" out="$2" payload
    payload="$(printf '{"jsonrpc":"2.0","id":%s,"method":"tools/call","params":{"name":"snapshot-interactive","arguments":{"limit":50}}}' "$id")"
    curl -fsS \
      -H "Authorization: Bearer ${JELLY_MCP_TOKEN}" \
      -H 'Content-Type: application/json' \
      -o "${out}" \
      -X POST "http://127.0.0.1:${MCP_PORT}/mcp" \
      --data "${payload}"
  }

  validate_mcp_response() {
    local out="$1" label="$2"
    if ! jq -e '.result.structuredContent.ok == true' "${out}" >/dev/null; then
      echo "FAIL: invalid MCP benchmark response: ${label}" >&2
      cat "${out}" >&2
      exit 1
    fi
  }

  start_mcp() {
    local persistent="$1"
    if [[ -n "${SERVER_PID}" ]]; then
      kill "${SERVER_PID}" >/dev/null 2>&1 || true
      wait "${SERVER_PID}" >/dev/null 2>&1 || true
    fi
    JELLY_MCP_ADDR="127.0.0.1:${MCP_PORT}"     JELLY_PUBLIC_URL="http://127.0.0.1:${MCP_PORT}"     JELLY_MCP_PERSISTENT_SESSION="${persistent}"     JELLY_PERF_LOG=1       "${BIN_DIR}/jelly-mcp" >/tmp/jelly-bench-mcp.log 2>&1 &
    SERVER_PID="$!"

    for _ in {1..50}; do
      if curl -fsS "http://127.0.0.1:${MCP_PORT}/health" >/dev/null 2>&1; then
        return 0
      fi
      sleep 0.1
    done
    cat /tmp/jelly-bench-mcp.log >&2 || true
    echo "benchmark MCP server did not become ready" >&2
    exit 1
  }

  benchmark_mcp_mode() {
    local persistent="$1" run out start end duration bytes connect_ms cdp_ms
    set_case large
    start_mcp "${persistent}"

    # Warm the HTTP/server path; persistent mode also establishes its CDP session here.
    out="$(mktemp)"
    mcp_call 0 "${out}"
    validate_mcp_response "${out}" "warmup persistent=${persistent}"
    rm -f "${out}"

    for ((run = 1; run <= RUNS; run++)); do
      out="$(mktemp)"
      rm -f "${PERF_LOG}"
      start="$(now_ns)"
      mcp_call "${run}" "${out}"
      end="$(now_ns)"
      validate_mcp_response "${out}" "persistent=${persistent} run=${run}"
      duration="$(( (end - start) / 1000000 ))"
      bytes="$(wc -c < "${out}" | tr -d ' ')"
      connect_ms="$(perf_sum browser.connect)"
      cdp_ms="$(perf_sum cdp.call Runtime.evaluate)"
      printf 'mcp\t%s\tlarge\tsnapshot-50\t%s\t%s\t%s\t%s\t%s\n' \
        "${persistent}" "${run}" "${duration}" "${bytes}" "${connect_ms}" "${cdp_ms}" >> "${RESULTS}"
      rm -f "${out}"
    done

    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" >/dev/null 2>&1 || true
    SERVER_PID=""
  }

  benchmark_mcp_mode 0
  benchmark_mcp_mode 1
else
  echo "Skipping MCP persistence benchmark: JELLY_MCP_TOKEN/JELLY_BOOTSTRAP_SECRET unavailable." >&2
fi

echo "Results: ${RESULTS}"
echo "Metadata: ${META}"
echo
python3 - "${RESULTS}" <<'PY'
import csv
import math
import statistics
import sys
from collections import defaultdict

path = sys.argv[1]
groups = defaultdict(list)
with open(path, newline='') as f:
    for row in csv.DictReader(f, delimiter='\t'):
        key = (row['scope'], row['mode'], row['case'], row['operation'])
        groups[key].append(row)

print(f"{'scope':<6} {'mode':<5} {'case':<10} {'operation':<24} {'runs':>6} {'avg':>8} {'p50':>8} {'p95':>8} {'connect':>8} {'cdp':>8} {'bytes':>10}")
for key in sorted(groups):
    rows = groups[key]
    durations = sorted(float(r['duration_ms']) for r in rows)
    p95 = durations[max(0, math.ceil(len(durations) * 0.95) - 1)]
    avg = statistics.fmean(durations)
    p50 = statistics.median(durations)
    connect = statistics.fmean(float(r['connect_ms']) for r in rows)
    cdp = statistics.fmean(float(r['cdp_ms']) for r in rows)
    size = statistics.fmean(float(r['bytes']) for r in rows)
    print(f"{key[0]:<6} {key[1]:<5} {key[2]:<10} {key[3]:<24} {len(rows):>6} {avg:>8.2f} {p50:>8.2f} {p95:>8.2f} {connect:>8.2f} {cdp:>8.2f} {size:>10.0f}")
PY
