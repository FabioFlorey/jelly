#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
# shellcheck source=scripts/lib/config.sh
source scripts/lib/config.sh
RUNTIME_ROOT="$CONFIG_RUNTIME_ROOT"
DEFAULT_BATCH="$CONFIG_TEST_DEFAULT_BATCH"

# shellcheck source=tests/suite/lib.sh
source tests/suite/lib.sh

for group_file in tests/suite/groups/*.sh; do
  [[ -e "$group_file" ]] || continue
  # shellcheck disable=SC1090
  source "$group_file"
done

# Classification augments the existing stable jt_register catalog without changing
# registration signatures or reinterpreting legacy 'kind' execution flags.
load_classification() {
  local id scope size focus execution environment review lifecycle triage related reason
  local file="tests/suite/config/classification.tsv"
  [[ -r "$file" ]] || { echo "missing classification catalog: $file" >&2; return 2; }
  while IFS=$'\t' read -r id scope size focus execution environment review lifecycle triage related reason; do
    [[ -z "$id" || "$id" == \#* ]] && continue
    [[ -n "${JT_FUNC[$id]:-}" ]] || { echo "classification references unknown test: $id" >&2; return 2; }
    [[ -z "${JT_SCOPE[$id]:-}" ]] || { echo "duplicate classification: $id" >&2; return 2; }
    case "$scope" in unit|component|system|cross-system|not-applicable|unclassified) ;; *) echo "invalid scope for $id: $scope" >&2; return 2 ;; esac
    case "$size" in small|medium|large|unclassified) ;; *) echo "invalid size for $id: $size" >&2; return 2 ;; esac
    case "$focus" in functional|non-functional|tooling|unclassified) ;; *) echo "invalid focus for $id: $focus" >&2; return 2 ;; esac
    case "$execution" in automated|manual|hybrid) ;; *) echo "invalid execution for $id: $execution" >&2; return 2 ;; esac
    case "$environment" in local-toolchain|native-browser|installed-mcp|public-web|docker|podman|remote-cdp|unclassified) ;; *) echo "invalid environment for $id: $environment" >&2; return 2 ;; esac
    case "$review" in provisional|reviewed) ;; *) echo "invalid review status for $id: $review" >&2; return 2 ;; esac
    case "$lifecycle" in active|deprecated|retired) ;; *) echo "invalid lifecycle for $id: $lifecycle" >&2; return 2 ;; esac
    case "$triage" in retain|consolidate|reassess) ;; *) echo "invalid triage for $id: $triage" >&2; return 2 ;; esac
    [[ -n "$reason" ]] || { echo "missing triage rationale for $id" >&2; return 2; }
    if [[ "$lifecycle" == retired && "$review" != reviewed ]]; then
      echo "retired test $id requires reviewed metadata" >&2; return 2
    fi
    JT_LIFECYCLE["$id"]="$lifecycle"
    JT_TRIAGE["$id"]="$triage"
    JT_RELATED_IDS["$id"]="$related"
    JT_TRIAGE_REASON["$id"]="$reason"
    JT_SCOPE["$id"]="$scope"
    JT_SIZE["$id"]="$size"
    JT_FOCUS["$id"]="$focus"
    JT_EXECUTION["$id"]="$execution"
    JT_ENVIRONMENT["$id"]="$environment"
    JT_REVIEW_STATUS["$id"]="$review"
  done < "$file"
  for id in "${JT_IDS[@]}"; do
    [[ -n "${JT_SCOPE[$id]:-}" ]] || { echo "missing classification for $id" >&2; return 2; }
  done
  local -a relations=()
  local other
  for id in "${JT_IDS[@]}"; do
    [[ "${JT_RELATED_IDS[$id]}" == "-" ]] && continue
    IFS=',' read -r -a relations <<< "${JT_RELATED_IDS[$id]}"
    for other in "${relations[@]}"; do
      [[ -n "${JT_FUNC[$other]:-}" && "$other" != "$id" ]] || {
        echo "invalid related test $id -> $other" >&2; return 2
      }
    done
  done
}
load_classification

usage() {
  cat <<'USAGE'
Usage:
  tests/suite/run.sh --test ID [--test ID ...]
  tests/suite/run.sh --group NAME [--group NAME ...]
  tests/suite/run.sh --batch NAME [--batch NAME ...]
  tests/suite/run.sh --suite NAME [--suite NAME ...]
  tests/suite/run.sh --scope system --environment native-browser
  tests/suite/run.sh --all
  tests/suite/run.sh --list
  tests/suite/run.sh --catalog [--json]

Options:
  --test ID            Run one stable test ID. Repeatable.
  --group NAME         Run every enabled test in a group. Repeatable.
  --batch NAME         Legacy group-based preset (config/batches.tsv).
  --suite NAME         Explicit test-ID selection (config/suites.tsv).
  --scope LEVEL        unit | component | system | cross-system | not-applicable | unclassified.
  --size SIZE          small | medium | large | unclassified.
  --focus FOCUS        functional | non-functional | tooling | unclassified.
  --environment ENV    local-toolchain | native-browser | installed-mcp | public-web | docker | podman | remote-cdp | unclassified.
  --execution MODE     automated | manual | hybrid.
  --review STATUS      provisional | reviewed classification.
  --lifecycle STATE    active | deprecated | retired.
  --triage ACTION      retain | consolidate | reassess.
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
declare -a WANT_SUITES=()
declare -a FILTER_SCOPE=() FILTER_SIZE=() FILTER_FOCUS=() FILTER_ENVIRONMENT=() FILTER_EXECUTION=() FILTER_REVIEW=() FILTER_LIFECYCLE=() FILTER_TRIAGE=()
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
    --suite) WANT_SUITES+=("${2:?missing suite name}"); shift 2 ;;
    --scope) FILTER_SCOPE+=("${2:?missing scope}"); shift 2 ;;
    --size) FILTER_SIZE+=("${2:?missing size}"); shift 2 ;;
    --focus) FILTER_FOCUS+=("${2:?missing focus}"); shift 2 ;;
    --environment) FILTER_ENVIRONMENT+=("${2:?missing environment}"); shift 2 ;;
    --execution) FILTER_EXECUTION+=("${2:?missing execution}"); shift 2 ;;
    --review) FILTER_REVIEW+=("${2:?missing review status}"); shift 2 ;;
    --lifecycle) FILTER_LIFECYCLE+=("${2:?missing lifecycle}"); shift 2 ;;
    --triage) FILTER_TRIAGE+=("${2:?missing triage}"); shift 2 ;;
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

# Reject unknown selector values rather than silently producing an empty report.
for value in "${FILTER_SCOPE[@]}"; do
  case "$value" in unit|component|system|cross-system|not-applicable|unclassified) ;; *) echo "invalid --scope: $value" >&2; exit 2;; esac
done
for value in "${FILTER_SIZE[@]}"; do
  case "$value" in small|medium|large|unclassified) ;; *) echo "invalid --size: $value" >&2; exit 2;; esac
done
for value in "${FILTER_FOCUS[@]}"; do
  case "$value" in functional|non-functional|tooling|unclassified) ;; *) echo "invalid --focus: $value" >&2; exit 2;; esac
done
for value in "${FILTER_ENVIRONMENT[@]}"; do
  case "$value" in local-toolchain|native-browser|installed-mcp|public-web|docker|podman|remote-cdp|unclassified) ;; *) echo "invalid --environment: $value" >&2; exit 2;; esac
done
for value in "${FILTER_EXECUTION[@]}"; do
  case "$value" in automated|manual|hybrid) ;; *) echo "invalid --execution: $value" >&2; exit 2;; esac
done
for value in "${FILTER_REVIEW[@]}"; do
  case "$value" in provisional|reviewed) ;; *) echo "invalid --review: $value" >&2; exit 2;; esac
done
for value in "${FILTER_LIFECYCLE[@]}"; do
  case "$value" in active|deprecated|retired) ;; *) echo "invalid --lifecycle: $value" >&2; exit 2;; esac
done
for value in "${FILTER_TRIAGE[@]}"; do
  case "$value" in retain|consolidate|reassess) ;; *) echo "invalid --triage: $value" >&2; exit 2;; esac
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

# A suite selects explicit stable IDs; test groups remain an independent domain axis.
for suite_name in "${WANT_SUITES[@]}"; do
  ids_csv="$(awk -F '\t' -v name="$suite_name" '$1==name{print $2}' tests/suite/config/suites.tsv)"
  [[ -n "$ids_csv" ]] || { echo "unknown suite: $suite_name" >&2; exit 2; }
  IFS=',' read -r -a suite_ids <<< "$ids_csv"
  WANT_TESTS+=("${suite_ids[@]}")
done

# Expand batches before catalog or executable selection, using the same ID union.
for batch in "${WANT_BATCHES[@]}"; do
  groups="$(awk -F '\t' -v b="$batch" '$1==b{print $2}' tests/suite/config/batches.tsv)"
  [[ -n "$groups" ]] || { echo "unknown batch: $batch" >&2; exit 2; }
  IFS=',' read -r -a expanded <<<"$groups"
  WANT_GROUPS+=("${expanded[@]}")
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

# All dimensional selectors are intersections; repeated values within one
# dimension form a union. An absent dimension imposes no restriction.
has_dimension_filters() {
  ((${#FILTER_SCOPE[@]} + ${#FILTER_SIZE[@]} + ${#FILTER_FOCUS[@]} + ${#FILTER_ENVIRONMENT[@]} + ${#FILTER_EXECUTION[@]} + ${#FILTER_REVIEW[@]} + ${#FILTER_LIFECYCLE[@]} + ${#FILTER_TRIAGE[@]} > 0))
}
matches_dimensions() {
  local id="$1"
  (("${#FILTER_SCOPE[@]}" == 0)) || contains_line "${JT_SCOPE[$id]}" "${FILTER_SCOPE[@]}" || return 1
  (("${#FILTER_SIZE[@]}" == 0)) || contains_line "${JT_SIZE[$id]}" "${FILTER_SIZE[@]}" || return 1
  (("${#FILTER_FOCUS[@]}" == 0)) || contains_line "${JT_FOCUS[$id]}" "${FILTER_FOCUS[@]}" || return 1
  (("${#FILTER_ENVIRONMENT[@]}" == 0)) || contains_line "${JT_ENVIRONMENT[$id]}" "${FILTER_ENVIRONMENT[@]}" || return 1
  (("${#FILTER_EXECUTION[@]}" == 0)) || contains_line "${JT_EXECUTION[$id]}" "${FILTER_EXECUTION[@]}" || return 1
  (("${#FILTER_REVIEW[@]}" == 0)) || contains_line "${JT_REVIEW_STATUS[$id]}" "${FILTER_REVIEW[@]}" || return 1
  (("${#FILTER_LIFECYCLE[@]}" == 0)) || contains_line "${JT_LIFECYCLE[$id]}" "${FILTER_LIFECYCLE[@]}" || return 1
  (("${#FILTER_TRIAGE[@]}" == 0)) || contains_line "${JT_TRIAGE[$id]}" "${FILTER_TRIAGE[@]}" || return 1
}

matches_selection() {
  local id="$1" selected=false
  $RUN_ALL && selected=true
  if ((${#WANT_TESTS[@]} == 0 && ${#WANT_GROUPS[@]} == 0)); then selected=true; fi
  contains_line "$id" "${WANT_TESTS[@]}" && selected=true
  contains_line "${JT_GROUP[$id]}" "${WANT_GROUPS[@]}" && selected=true
  $selected || return 1
  matches_dimensions "$id"
}

test_enabled() {
  local id="$1"
  local group="${JT_GROUP[$1]}"
  [[ "${JT_LIFECYCLE[$id]}" == retired ]] && return 1
  $INCLUDE_DISABLED && return 0
  contains_line "$id" "${DISABLED_TESTS[@]}" && return 1
  contains_line "$group" "${DISABLED_GROUPS[@]}" && return 1
  return 0
}

if $MODE_LIST; then
  printf 'ID\tGROUP\tKIND\tENABLED\tNAME\tSCOPE\tSIZE\tFOCUS\tENVIRONMENT\tREVIEW\tLIFECYCLE\tTRIAGE\n'
  for id in "${JT_IDS[@]}"; do
    matches_selection "$id" || continue
    enabled=true
    test_enabled "$id" || enabled=false
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "${JT_GROUP[$id]}" "${JT_KIND[$id]}" "$enabled" "${JT_NAME[$id]}" "${JT_SCOPE[$id]}" "${JT_SIZE[$id]}" "${JT_FOCUS[$id]}" "${JT_ENVIRONMENT[$id]}" "${JT_REVIEW_STATUS[$id]}" "${JT_LIFECYCLE[$id]}" "${JT_TRIAGE[$id]}"
  done
  exit 0
fi

emit_catalog_tsv() {
  printf 'ID\tGROUP\tNAME\tDESCRIPTION\tPRECONDITIONS\tINPUT\tEXPECTED_OUTPUT\tKIND\tENABLED\tSCOPE\tSIZE\tFOCUS\tEXECUTION\tENVIRONMENT\tREVIEW_STATUS\tLIFECYCLE\tTRIAGE\tRELATED_IDS\tTRIAGE_REASON\n'
  local id enabled
  for id in "${JT_IDS[@]}"; do
    matches_selection "$id" || continue
    enabled=true
    test_enabled "$id" || enabled=false
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$id" "${JT_GROUP[$id]}" "${JT_NAME[$id]}" "${JT_DESCRIPTION[$id]}" \
      "${JT_PRECONDITIONS[$id]}" "${JT_INPUT[$id]}" "${JT_EXPECTED[$id]}" \
      "${JT_KIND[$id]}" "$enabled" "${JT_SCOPE[$id]}" "${JT_SIZE[$id]}" "${JT_FOCUS[$id]}" \
      "${JT_EXECUTION[$id]}" "${JT_ENVIRONMENT[$id]}" "${JT_REVIEW_STATUS[$id]}" \
      "${JT_LIFECYCLE[$id]}" "${JT_TRIAGE[$id]}" "${JT_RELATED_IDS[$id]}" "${JT_TRIAGE_REASON[$id]}"
  done
}

emit_catalog_json() {
  local id enabled
  local tmp
  tmp="$(mktemp)"
  : > "$tmp"
  for id in "${JT_IDS[@]}"; do
    matches_selection "$id" || continue
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
      --arg scope "${JT_SCOPE[$id]}" \
      --arg size "${JT_SIZE[$id]}" \
      --arg focus "${JT_FOCUS[$id]}" \
      --arg execution "${JT_EXECUTION[$id]}" \
      --arg environment "${JT_ENVIRONMENT[$id]}" \
      --arg review_status "${JT_REVIEW_STATUS[$id]}" \
      --arg lifecycle "${JT_LIFECYCLE[$id]}" \
      --arg triage "${JT_TRIAGE[$id]}" \
      --arg related_ids "${JT_RELATED_IDS[$id]}" \
      --arg triage_reason "${JT_TRIAGE_REASON[$id]}" \
      --argjson enabled "$enabled" \
      '{id:$id,group:$group,name:$name,description:$description,preconditions:$preconditions,input:$input,expected_output:$expected_output,kind:$kind,enabled:$enabled,scope:$scope,size:$size,focus:$focus,execution:$execution,environment:$environment,review_status:$review_status,lifecycle:$lifecycle,triage:$triage,related_ids:($related_ids|if .=="-" then [] else split(",") end),triage_reason:$triage_reason}' >> "$tmp"
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

if ! $RUN_ALL && ((${#WANT_TESTS[@]} == 0)) && ((${#WANT_GROUPS[@]} == 0)); then
  if has_dimension_filters; then
    RUN_ALL=true
  else
    groups="$(awk -F '\t' -v b="$DEFAULT_BATCH" '$1==b{print $2}' tests/suite/config/batches.tsv)"
    [[ -n "$groups" ]] || { echo "unknown configured default batch: $DEFAULT_BATCH" >&2; exit 2; }
    IFS=',' read -r -a WANT_GROUPS <<<"$groups"
  fi
fi

declare -a SELECTED=()
for id in "${JT_IDS[@]}"; do
  matches_selection "$id" || continue
  SELECTED+=("$id")
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
printf 'TEST_RUN_UNIQUE_ID\tTEST_ID\tGROUP\tNAME\tDESCRIPTION\tPRECONDITIONS\tINPUT\tEXPECTED_OUTPUT\tSTATUS\tDURATION_MS\tLOG\tSCOPE\tSIZE\tFOCUS\tEXECUTION\tENVIRONMENT\tREVIEW_STATUS\tLIFECYCLE\tTRIAGE\tRELATED_IDS\tTRIAGE_REASON\n' > "$REPORT_TSV"

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
    --arg group "${JT_GROUP[$id]}" \
    --arg kind "${JT_KIND[$id]}" \
    --arg status "$status" \
    --argjson duration_ms "$duration_ms" \
    --arg log "$log" \
    --arg scope "${JT_SCOPE[$id]}" \
    --arg size "${JT_SIZE[$id]}" \
    --arg focus "${JT_FOCUS[$id]}" \
    --arg execution "${JT_EXECUTION[$id]}" \
    --arg environment "${JT_ENVIRONMENT[$id]}" \
    --arg review_status "${JT_REVIEW_STATUS[$id]}" \
    --arg lifecycle "${JT_LIFECYCLE[$id]}" \
    --arg triage "${JT_TRIAGE[$id]}" \
    '{test_run_unique_id:$test_run_unique_id,test_id:$test_id,group:$group,kind:$kind,status:$status,duration_ms:$duration_ms,log:$log,scope:$scope,size:$size,focus:$focus,execution:$execution,environment:$environment,review_status:$review_status,lifecycle:$lifecycle,triage:$triage}' \
    >> "$RESULTS_JSONL"

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$RUN_ID" "$id" "${JT_GROUP[$id]}" "${JT_NAME[$id]}" "${JT_DESCRIPTION[$id]}" \
    "${JT_PRECONDITIONS[$id]}" "${JT_INPUT[$id]}" "${JT_EXPECTED[$id]}" \
    "$status" "$duration_ms" "$log" "${JT_SCOPE[$id]}" "${JT_SIZE[$id]}" "${JT_FOCUS[$id]}" \
    "${JT_EXECUTION[$id]}" "${JT_ENVIRONMENT[$id]}" "${JT_REVIEW_STATUS[$id]}" \
    "${JT_LIFECYCLE[$id]}" "${JT_TRIAGE[$id]}" "${JT_RELATED_IDS[$id]}" "${JT_TRIAGE_REASON[$id]}" >> "$REPORT_TSV"
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
