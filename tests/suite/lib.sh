#!/usr/bin/env bash

: "${JELLY_REPO_ROOT:=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
: "${JELLY_BIN_DIR:=/data/.jelly-build/debug}"
: "${CARGO_TARGET_DIR:=/data/.jelly-build}"

declare -ag JT_IDS=()
declare -Ag JT_GROUP=()
declare -Ag JT_NAME=()
declare -Ag JT_DESCRIPTION=()
declare -Ag JT_PRECONDITIONS=()
declare -Ag JT_INPUT=()
declare -Ag JT_EXPECTED=()
declare -Ag JT_KIND=()
declare -Ag JT_FUNC=()

jt_register() {
  local id="$1" group="$2" name="$3" description="$4" preconditions="$5" input="$6" expected="$7" kind="$8" fn="$9"
  if [[ -n "${JT_FUNC[$id]:-}" ]]; then
    echo "duplicate test id: $id" >&2
    return 2
  fi
  JT_IDS+=("$id")
  JT_GROUP["$id"]="$group"
  JT_NAME["$id"]="$name"
  JT_DESCRIPTION["$id"]="$description"
  JT_PRECONDITIONS["$id"]="$preconditions"
  JT_INPUT["$id"]="$input"
  JT_EXPECTED["$id"]="$expected"
  JT_KIND["$id"]="$kind"
  JT_FUNC["$id"]="$fn"
}

jt_fail() {
  echo "ASSERTION FAILED: $*" >&2
  return 1
}

jt_assert_eq() {
  local actual="$1" expected="$2" message="$3"
  [[ "$actual" == "$expected" ]] || jt_fail "$message: expected '$expected', got '$actual'"
}

jt_assert_ne() {
  local actual="$1" unexpected="$2" message="$3"
  [[ "$actual" != "$unexpected" ]] || jt_fail "$message: unexpectedly got '$actual'"
}

jt_assert_nonempty() {
  local actual="$1" message="$2"
  [[ -n "$actual" && "$actual" != "null" ]] || jt_fail "$message"
}

jt_assert_true() {
  jt_assert_eq "$1" "true" "$2"
}

jt_assert_false() {
  jt_assert_eq "$1" "false" "$2"
}

jt_assert_ge() {
  local actual="$1" minimum="$2" message="$3"
  awk -v a="$actual" -v b="$minimum" 'BEGIN { exit !(a >= b) }' || jt_fail "$message: expected >= $minimum, got $actual"
}

jt_assert_gt() {
  local actual="$1" minimum="$2" message="$3"
  awk -v a="$actual" -v b="$minimum" 'BEGIN { exit !(a > b) }' || jt_fail "$message: expected > $minimum, got $actual"
}

jt_assert_lt() {
  local actual="$1" maximum="$2" message="$3"
  awk -v a="$actual" -v b="$maximum" 'BEGIN { exit !(a < b) }' || jt_fail "$message: expected < $maximum, got $actual"
}

jt_expect_failure() {
  local message="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    jt_fail "$message: command unexpectedly succeeded"
  fi
}

jt_expect_success() {
  local message="$1"
  shift
  "$@" >/dev/null 2>&1 || jt_fail "$message: command failed"
}

jt_json_eq() {
  local json="$1" filter="$2" expected="$3" message="$4"
  local actual
  actual="$(jq -r "$filter" <<<"$json")"
  jt_assert_eq "$actual" "$expected" "$message"
}

jt_wait_until() {
  local timeout_ms="$1" interval_ms="$2" description="$3"
  shift 3
  local elapsed=0
  while (( elapsed <= timeout_ms )); do
    if "$@"; then
      return 0
    fi
    sleep "$(awk -v ms="$interval_ms" 'BEGIN { printf "%.3f", ms/1000 }')"
    elapsed=$((elapsed + interval_ms))
  done
  jt_fail "timed out waiting for $description after ${timeout_ms}ms"
}

jt_wait_browser_service_gone() {
  local load
  for _ in {1..80}; do
    load="$(systemctl --user show jelly-browser.service --property=LoadState --value 2>/dev/null || true)"
    if [[ -z "$load" || "$load" == "not-found" ]]; then
      return 0
    fi
    sleep 0.1
  done
  jt_fail "jelly-browser.service did not unload after browser close"
}

jt_close_browser() {
  "${JELLY_BIN_DIR}/agent-close-browser" >/dev/null 2>&1 || true
  jt_wait_browser_service_gone
}

jt_open_url() {
  local url="$1" opened=false current
  jt_close_browser
  for _ in {1..6}; do
    if "${JELLY_BIN_DIR}/agent-open-browser" --headless "$url" >/dev/null 2>&1; then
      for _ in {1..40}; do
        current="$("${JELLY_BIN_DIR}/agent-evaluate-js" '({ready:document.readyState,url:location.href})' 2>/dev/null || true)"
        if jq -e '.ready == "complete" and (.url|length>0)' >/dev/null 2>&1 <<<"$current"; then
          opened=true
          break
        fi
        sleep 0.1
      done
    fi
    $opened && break
    "${JELLY_BIN_DIR}/agent-close-browser" >/dev/null 2>&1 || true
    jt_wait_browser_service_gone || true
    sleep 0.2
  done
  jt_assert_eq "$opened" "true" "browser must open a ready document: $url"
}

jt_open_fixture() {
  local fixture="$1" expected actual
  expected="file://${JELLY_REPO_ROOT}/tests/fixtures/${fixture}"
  jt_open_url "$expected"
  actual="$("${JELLY_BIN_DIR}/agent-evaluate-js" 'location.href' | jq -r '.')"
  jt_assert_eq "$actual" "$expected" "browser must load requested fixture"
}

jt_snapshot() {
  local limit="${1:-200}" offset="${2:-0}"
  JELLY_PAGE_RUNTIME=1 "${JELLY_BIN_DIR}/agent-snapshot-interactive" "$limit" "$offset"
}

jt_ref_for_name() {
  local name="$1"
  jq -r --arg name "$name" '.[] | select(.name == $name) | .ref' | head -1
}

jt_find() {
  local query="$1" limit="${2:-20}" offset="${3:-0}"
  JELLY_PAGE_RUNTIME=1 "${JELLY_BIN_DIR}/agent-find-interactive" "$query" "$limit" "$offset"
}

jt_runtime_eval() {
  "${JELLY_BIN_DIR}/agent-evaluate-js" "$1"
}

jt_with_browser_cleanup() {
  trap 'jt_close_browser' EXIT
}
