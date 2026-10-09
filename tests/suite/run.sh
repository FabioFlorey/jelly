#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
# shellcheck source=scripts/config.sh
source scripts/config.sh
RUNTIME_ROOT="$CONFIG_RUNTIME_ROOT"
DEFAULT_BATCH="$CONFIG_TEST_DEFAULT_BATCH"

# shellcheck source=tests/suite/lib.sh
source tests/suite/lib.sh

for group_file in tests/suite/groups/*.sh; do
  [[ -e "$group_file" ]] || continue
  # shellcheck disable=SC1090
  source "$group_file"
done

usage() {
  cat <<'USAGE'
Usage:
  tests/suite/run.sh --test ID [--test ID ...]
  tests/suite/run.sh --group NAME [--group NAME ...]
  tests/suite/run.sh --batch NAME [--batch NAME ...]
  tests/suite/run.sh --all
  tests/suite/run.sh --list
  tests/suite/run.sh --catalog [--json]

Options:
  --test ID            Run one stable test ID. Repeatable.
  --group NAME         Run every enabled test in a group. Repeatable.
  --batch NAME         Run groups defined in config/batches.tsv. Repeatable.
  --all                Run every enabled test.
  --include-disabled   Ignore disabled-tests.txt / disabled-groups.txt.
  --results-dir DIR    Override the configured runtime test-runs directory.
  --list               Show IDs, groups, kinds, names and enabled state.
  --catalog            Print the complete test catalog.
  --json               Use JSON for --catalog.
  --help               Show this help.

With no selector, the configured default batch is run.
USAGE
}

declare -a WANT_TESTS=()
declare -a WANT_GROUPS=()
declare -a WANT_BATCHES=()
RUN_ALL=false
INCLUDE_DISABLED=false
MODE_LIST=false
MODE_CATALOG=false
CATALOG_JSON=false
RESULTS_ROOT="$RUNTIME_ROOT/test-runs"

while (($#)); do
  case "$1" in
    --test) WANT_TESTS+=("${2:?missing test ID}"); shift 2 ;;
    --group) WANT_GROUPS+=("${2:?missing group name}"); shift 2 ;;
    --batch) WANT_BATCHES+=("${2:?missing batch name}"); shift 2 ;;
    --all) RUN_ALL=true; shift ;;
    --include-disabled) INCLUDE_DISABLED=true; shift ;;
    --results-dir) RESULTS_ROOT="${2:?missing results dir}"; shift 2 ;;
    --list) MODE_LIST=true; shift ;;
    --catalog) MODE_CATALOG=true; shift ;;
    --json) CATALOG_JSON=true; shift ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

# Catalog and listing are read-only; do not build the Rust parser or load
# deployment secrets until an actual execution run has been requested.
if ! $MODE_LIST && ! $MODE_CATALOG; then
  jelly_load_env
fi

read_set_file() {
  local file="$1"
  [[ -f "$file" ]] || return 0
  sed -e 's/[[:space:]]*#.*$//' -e '/^[[:space:]]*$/d' "$file"
}

mapfile -t DISABLED_TESTS < <(read_set_file tests/suite/config/disabled-tests.txt)
mapfile -t DISABLED_GROUPS < <(read_set_file tests/suite/config/disabled-groups.txt)

contains_line() {
  local needle="$1"
  shift
  local item
  for item in "$@"; do [[ "$item" == "$needle" ]] && return 0; done
  return 1
}

test_enabled() {
  local id="$1"
  local group="${JT_GROUP[$1]}"
  $INCLUDE_DISABLED && return 0
  contains_line "$id" "${DISABLED_TESTS[@]}" && return 1
  contains_line "$group" "${DISABLED_GROUPS[@]}" && return 1
  return 0
}

if $MODE_LIST; then
  printf 'ID\tGROUP\tKIND\tENABLED\tNAME\n'
  for id in "${JT_IDS[@]}"; do
    enabled=true
    test_enabled "$id" || enabled=false
    printf '%s\t%s\t%s\t%s\t%s\n' "$id" "${JT_GROUP[$id]}" "${JT_KIND[$id]}" "$enabled" "${JT_NAME[$id]}"
  done
  exit 0
fi

emit_catalog_tsv() {
  printf 'ID\tGROUP\tNAME\tDESCRIPTION\tPRECONDITIONS\tINPUT\tEXPECTED_OUTPUT\tKIND\tENABLED\n'
  local id enabled
  for id in "${JT_IDS[@]}"; do
    enabled=true
    test_enabled "$id" || enabled=false
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$id" "${JT_GROUP[$id]}" "${JT_NAME[$id]}" "${JT_DESCRIPTION[$id]}" \
      "${JT_PRECONDITIONS[$id]}" "${JT_INPUT[$id]}" "${JT_EXPECTED[$id]}" \
      "${JT_KIND[$id]}" "$enabled"
  done
}

emit_catalog_json() {
  local id enabled
  local tmp
  tmp="$(mktemp)"
  : > "$tmp"
  for id in "${JT_IDS[@]}"; do
    enabled=true
    test_enabled "$id" || enabled=false
    jq -cn \
      --arg id "$id" \
      --arg group "${JT_GROUP[$id]}" \
      --arg name "${JT_NAME[$id]}" \
      --arg description "${JT_DESCRIPTION[$id]}" \
      --arg preconditions "${JT_PRECONDITIONS[$id]}" \
      --arg input "${JT_INPUT[$id]}" \
      --arg expected_output "${JT_EXPECTED[$id]}" \
      --arg kind "${JT_KIND[$id]}" \
      --argjson enabled "$enabled" \
      '{id:$id,group:$group,name:$name,description:$description,preconditions:$preconditions,input:$input,expected_output:$expected_output,kind:$kind,enabled:$enabled}' >> "$tmp"
  done
  jq -s '.' "$tmp"
  rm -f "$tmp"
}

if $MODE_CATALOG; then
  if $CATALOG_JSON; then emit_catalog_json; else emit_catalog_tsv; fi
  exit 0
fi

# Jelly owns one shared browser/service state. Concurrent suite runs would close or
# retarget the browser underneath each other, so serialize executable runs.
mkdir -p "$RUNTIME_ROOT"
TEST_LOCK_FILE="$RUNTIME_ROOT/test-suite.lock"
exec 9>"$TEST_LOCK_FILE"
if ! flock -n 9; then
  echo "another Jelly test-suite run is already active (lock: $TEST_LOCK_FILE)" >&2
  exit 75
fi

CONFIG_BASELINE="$(mktemp)"
cp "$CONFIG_FILE" "$CONFIG_BASELINE"
restore_test_config() {
  cp "$CONFIG_BASELINE" "$CONFIG_FILE"
  rm -f "$CONFIG_BASELINE"
}
trap restore_test_config EXIT

if ((${#WANT_BATCHES[@]})); then
  for batch in "${WANT_BATCHES[@]}"; do
    groups="$(awk -F '\t' -v b="$batch" '$1==b{print $2}' tests/suite/config/batches.tsv)"
    [[ -n "$groups" ]] || { echo "unknown batch: $batch" >&2; exit 2; }
    IFS=',' read -r -a expanded <<<"$groups"
    WANT_GROUPS+=("${expanded[@]}")
  done
fi

if ! $RUN_ALL && ((${#WANT_TESTS[@]} == 0)) && ((${#WANT_GROUPS[@]} == 0)); then
  groups="$(awk -F '\t' -v b="$DEFAULT_BATCH" '$1==b{print $2}' tests/suite/config/batches.tsv)"
  [[ -n "$groups" ]] || { echo "unknown configured default batch: $DEFAULT_BATCH" >&2; exit 2; }
  IFS=',' read -r -a WANT_GROUPS <<<"$groups"
fi

declare -a SELECTED=()
for id in "${JT_IDS[@]}"; do
  selected=false
  $RUN_ALL && selected=true
  contains_line "$id" "${WANT_TESTS[@]}" && selected=true
  contains_line "${JT_GROUP[$id]}" "${WANT_GROUPS[@]}" && selected=true
  $selected || continue
  SELECTED+=("$id")
done

for requested in "${WANT_TESTS[@]}"; do
  [[ -n "${JT_FUNC[$requested]:-}" ]] || { echo "unknown test ID: $requested" >&2; exit 2; }
done
for requested in "${WANT_GROUPS[@]}"; do
  found=false
  for id in "${JT_IDS[@]}"; do
    [[ "${JT_GROUP[$id]}" == "$requested" ]] && { found=true; break; }
  done
  $found || { echo "unknown group: $requested" >&2; exit 2; }
done

((${#SELECTED[@]})) || { echo "selection contains no tests" >&2; exit 2; }

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$(cat /proc/sys/kernel/random/uuid | cut -c1-8)"
RUN_DIR="$RESULTS_ROOT/$RUN_ID"
mkdir -p "$RUN_DIR/logs"
RESULTS_JSONL="$RUN_DIR/results.jsonl"
REPORT_TSV="$RUN_DIR/report.tsv"
: > "$RESULTS_JSONL"

jq -cn \
  --arg run_id "$RUN_ID" \
  --arg started_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg repo "$ROOT" \
  --arg git_commit "$(git rev-parse HEAD)" \
  --arg branch "$(git branch --show-current)" \
  '{test_run_unique_id:$run_id,started_at:$started_at,repository:$repo,git_commit:$git_commit,branch:$branch}' \
  > "$RUN_DIR/run.json"

emit_catalog_tsv > "$RUN_DIR/catalog.tsv"
emit_catalog_json > "$RUN_DIR/catalog.json"
printf 'TEST_RUN_UNIQUE_ID\tTEST_ID\tGROUP\tNAME\tDESCRIPTION\tPRECONDITIONS\tINPUT\tEXPECTED_OUTPUT\tSTATUS\tDURATION_MS\tLOG\n' > "$REPORT_TSV"

needs_bins=false
for id in "${SELECTED[@]}"; do
  case "${JT_KIND[$id]}" in
    browser|network|mcp) needs_bins=true ;;
  esac
done
if $needs_bins; then
  echo "Preflight: building current Jelly binaries..."
  if ! CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo build --bins --quiet >"$RUN_DIR/preflight-build.log" 2>&1; then
    echo "Preflight build failed: $RUN_DIR/preflight-build.log" >&2
    exit 1
  fi
fi

pass=0
fail=0
disabled=0
for id in "${SELECTED[@]}"; do
  status=""
  duration_ms=0
  log="$RUN_DIR/logs/$id.log"
  : > "$log"

  cp "$CONFIG_BASELINE" "$CONFIG_FILE"

  if ! test_enabled "$id"; then
    status="DISABLED"
    ((disabled+=1))
  else
    echo "RUN  $id  ${JT_NAME[$id]}"
    start_ns="$(date +%s%N)"
    set +e
    (
      set -euo pipefail
      export REPO_ROOT="$ROOT"
      export BIN_DIR
      export CARGO_TARGET_DIR
      cd "$ROOT"
      "${JT_FUNC[$id]}"
    ) >"$log" 2>&1
    code=$?
    set -e
    end_ns="$(date +%s%N)"
    duration_ms=$(( (end_ns - start_ns) / 1000000 ))
    if (( code == 0 )); then
      status="PASS"
      ((pass+=1))
      echo "PASS $id  ($duration_ms ms)"
    else
      status="FAIL"
      ((fail+=1))
      echo "FAIL $id  ($duration_ms ms)"
      tail -n 30 "$log" | sed 's/^/     /'
    fi
  fi

  jq -cn \
    --arg test_run_unique_id "$RUN_ID" \
    --arg test_id "$id" \
    --arg status "$status" \
    --argjson duration_ms "$duration_ms" \
    --arg log "$log" \
    '{test_run_unique_id:$test_run_unique_id,test_id:$test_id,status:$status,duration_ms:$duration_ms,log:$log}' \
    >> "$RESULTS_JSONL"

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$RUN_ID" "$id" "${JT_GROUP[$id]}" "${JT_NAME[$id]}" "${JT_DESCRIPTION[$id]}" \
    "${JT_PRECONDITIONS[$id]}" "${JT_INPUT[$id]}" "${JT_EXPECTED[$id]}" \
    "$status" "$duration_ms" "$log" >> "$REPORT_TSV"
done

jq -s \
  --arg run_id "$RUN_ID" \
  --arg finished_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --argjson pass "$pass" \
  --argjson fail "$fail" \
  --argjson disabled "$disabled" \
  '{test_run_unique_id:$run_id,finished_at:$finished_at,pass:$pass,fail:$fail,disabled:$disabled,tests:.}' \
  "$RESULTS_JSONL" > "$RUN_DIR/summary.json"

echo
echo "Test Run Unique ID: $RUN_ID"
echo "PASS=$pass FAIL=$fail DISABLED=$disabled"
echo "Report: $REPORT_TSV"
echo "Summary: $RUN_DIR/summary.json"

(( fail == 0 ))
