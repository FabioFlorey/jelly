#!/usr/bin/env bash

rollback_load_env() {
  if [[ -f .env ]]; then
    set -a
    # shellcheck disable=SC1091
    source .env
    set +a
  fi
  : "${JELLY_MCP_TOKEN:?JELLY_MCP_TOKEN is required for rollback tests}"
  : "${JELLY_BOOTSTRAP_SECRET:?JELLY_BOOTSTRAP_SECRET is required for rollback tests}"
}

rollback_wait_browser_gone() {
  for _ in {1..80}; do
    local load
    load="$(systemctl --user show jelly-browser.service --property=LoadState --value 2>/dev/null || true)"
    [[ -z "$load" || "$load" == "not-found" ]] && return 0
    sleep 0.1
  done
  jt_fail "jelly-browser.service did not unload"
}

rollback_close_browser() {
  jt_close_browser
  rollback_wait_browser_gone
}

rollback_stop_server() {
  if [[ -n "${RB_PID:-}" ]]; then
    kill "$RB_PID" >/dev/null 2>&1 || true
    wait "$RB_PID" >/dev/null 2>&1 || true
    RB_PID=""
  fi
}

rollback_cleanup() {
  rollback_stop_server
  rollback_close_browser >/dev/null 2>&1 || true
}

rollback_start_server() {
  local port="$1" persistent="$2" runtime="$3" limit="${4:-17}"
  rollback_stop_server
  RB_PORT="$port"
  JELLY_MCP_ADDR="127.0.0.1:${port}" \
  JELLY_PUBLIC_URL="http://127.0.0.1:${port}" \
  JELLY_MCP_SURFACE="large-surface" \
  JELLY_MCP_PERSISTENT_SESSION="$persistent" \
  JELLY_PAGE_RUNTIME="$runtime" \
  JELLY_SNAPSHOT_LIMIT="$limit" \
    "$JELLY_BIN_DIR/jelly-mcp" >"/tmp/jelly-suite-rollback-${port}.log" 2>&1 &
  RB_PID=$!
  for _ in {1..80}; do
    curl -fsS "http://127.0.0.1:${port}/health" >/dev/null 2>&1 && return 0
    sleep 0.1
  done
  cat "/tmp/jelly-suite-rollback-${port}.log" >&2 || true
  jt_fail "rollback MCP server failed to start"
}

rollback_call() {
  local id="$1" name="$2" args="$3" payload
  payload="$(jq -cn --argjson id "$id" --arg name "$name" --argjson arguments "$args" '{jsonrpc:"2.0",id:$id,method:"tools/call",params:{name:$name,arguments:$arguments}}')"
  curl -fsS -H "Authorization: Bearer $JELLY_MCP_TOKEN" -H 'Content-Type: application/json' -X POST "http://127.0.0.1:${RB_PORT}/mcp" --data "$payload"
}

rollback_expect_ok() {
  jq -e '.result.structuredContent.ok == true' >/dev/null <<<"$1" || jt_fail "$2"
}

rollback_open_small() {
  rollback_close_browser
  "$JELLY_BIN_DIR/agent-open-browser" --headless "file://${JELLY_REPO_ROOT}/tests/fixtures/browser-perf.html" >/dev/null
  jt_runtime_eval "window.__jellyBench.setCase('small')" >/dev/null
}

rollback_matrix() {
  rollback_load_env
  trap rollback_cleanup EXIT
  local persistent runtime port=18820 r data ref info click search runtime_state dom_refs
  for persistent in 0 1; do
    for runtime in 0 1; do
      rollback_open_small
      rollback_start_server "$port" "$persistent" "$runtime" 17
      r="$(rollback_call 1 snapshot-interactive '{}')"; rollback_expect_ok "$r" "matrix snapshot failed"
      data="$(jq -c '.result.structuredContent.data' <<<"$r")"
      jt_assert_eq "$(jq 'length' <<<"$data")" "17" "matrix default limit must be 17"
      r="$(rollback_call 2 snapshot-interactive '{"limit":5}')"; rollback_expect_ok "$r" "matrix explicit snapshot failed"
      data="$(jq -c '.result.structuredContent.data' <<<"$r")"
      jt_assert_eq "$(jq 'length' <<<"$data")" "5" "explicit limit must override env"
      ref="$(jq -r '.[]|select(.name=="small action 3")|.ref' <<<"$data")"
      jt_assert_nonempty "$ref" "matrix target ref missing"
      info="$(rollback_call 3 element-info "$(jq -cn --arg target "$ref" '{target:$target}')")"; rollback_expect_ok "$info" "ref resolution failed"
      jt_assert_eq "$(jq -r '.result.structuredContent.data.tag' <<<"$info")" "button" "matrix ref must resolve button"
      click="$(rollback_call 4 click "$(jq -cn --arg target "$ref" '{target:$target}')")"; rollback_expect_ok "$click" "matrix click failed"
      jt_assert_true "$(jq -r '.result.structuredContent.data.performed' <<<"$click")" "matrix click must perform"
      search="$(rollback_call 5 find-interactive '{"query":"small action 3","limit":5}')"; rollback_expect_ok "$search" "matrix semantic search failed"
      jt_assert_eq "$(jq -r '.result.structuredContent.data[0].name' <<<"$search")" "small action 3" "matrix search must preserve target"
      runtime_state="$(jt_runtime_eval '!!globalThis.__jellyRuntimeV1')"
      dom_refs="$(jt_runtime_eval "document.querySelectorAll('[data-jelly-ref]').length")"
      if [[ "$runtime" == 1 ]]; then
        jt_assert_true "$runtime_state" "runtime=1 must install page runtime"
        jt_assert_eq "$dom_refs" "0" "runtime=1 must avoid DOM refs"
      else
        jt_assert_false "$runtime_state" "runtime=0 must dispose page runtime"
        jt_assert_gt "$dom_refs" "0" "runtime=0 must use DOM refs"
      fi
      rollback_stop_server
      rollback_close_browser
      port=$((port+1))
    done
  done
}

rollback_transition_one() {
  local persistent="$1" base="$2" r data runtime_ref legacy_ref new_ref stale
  rollback_open_small
  rollback_start_server "$base" "$persistent" 1 17
  r="$(rollback_call 101 snapshot-interactive '{"limit":5}')"; rollback_expect_ok "$r" "runtime-on snapshot failed"
  data="$(jq -c '.result.structuredContent.data' <<<"$r")"
  runtime_ref="$(jq -r '.[]|select(.name=="small action 3")|.ref' <<<"$data")"
  [[ "$runtime_ref" == @e*-* ]] || jt_fail "runtime-on ref must be tokenized"
  rollback_stop_server

  rollback_start_server "$((base+1))" "$persistent" 0 17
  r="$(rollback_call 102 snapshot-interactive '{"limit":5}')"; rollback_expect_ok "$r" "runtime-off snapshot failed"
  data="$(jq -c '.result.structuredContent.data' <<<"$r")"
  legacy_ref="$(jq -r '.[]|select(.name=="small action 3")|.ref' <<<"$data")"
  [[ "$legacy_ref" =~ ^@e[0-9]+$ ]] || jt_fail "runtime-off ref must be legacy numeric"
  jt_assert_false "$(jt_runtime_eval '!!globalThis.__jellyRuntimeV1')" "runtime-off transition must dispose runtime"
  jt_assert_gt "$(jt_runtime_eval "document.querySelectorAll('[data-jelly-ref]').length")" "0" "runtime-off transition must use DOM refs"
  stale="$(rollback_call 103 element-info "$(jq -cn --arg target "$runtime_ref" '{target:$target}')")"
  if jq -e '.result.structuredContent.ok == true' >/dev/null <<<"$stale"; then jt_fail "old runtime ref resolved after rollback"; fi
  rollback_stop_server

  rollback_start_server "$((base+2))" "$persistent" 1 17
  r="$(rollback_call 104 snapshot-interactive '{"limit":5}')"; rollback_expect_ok "$r" "runtime-reenabled snapshot failed"
  data="$(jq -c '.result.structuredContent.data' <<<"$r")"
  new_ref="$(jq -r '.[]|select(.name=="small action 3")|.ref' <<<"$data")"
  [[ "$new_ref" == @e*-* ]] || jt_fail "re-enabled runtime ref must be tokenized"
  jt_assert_ne "$new_ref" "$runtime_ref" "re-enabled runtime must use fresh namespace"
  stale="$(rollback_call 105 element-info "$(jq -cn --arg target "$legacy_ref" '{target:$target}')")"
  if jq -e '.result.structuredContent.ok == true' >/dev/null <<<"$stale"; then jt_fail "old legacy ref resolved after runtime re-enable"; fi
  rollback_stop_server
  rollback_close_browser
}

rollback_transitions() {
  rollback_load_env
  trap rollback_cleanup EXIT
  rollback_transition_one 0 18830
  rollback_transition_one 1 18840
}

rollback_limit_zero() {
  rollback_load_env
  trap rollback_cleanup EXIT
  local runtime port=18850 r data
  for runtime in 0 1; do
    rollback_close_browser
    "$JELLY_BIN_DIR/agent-open-browser" --headless "file://${JELLY_REPO_ROOT}/tests/fixtures/browser-perf.html" >/dev/null
    jt_runtime_eval "window.__jellyBench.setCase('large')" >/dev/null
    rollback_start_server "$port" 1 "$runtime" 0
    r="$(rollback_call 201 snapshot-interactive '{}')"; rollback_expect_ok "$r" "limit-zero unlimited snapshot failed"
    data="$(jq -c '.result.structuredContent.data' <<<"$r")"
    jt_assert_ge "$(jq 'length' <<<"$data")" "1000" "JELLY_SNAPSHOT_LIMIT=0 must restore unlimited results"
    r="$(rollback_call 202 snapshot-interactive '{"limit":7}')"; rollback_expect_ok "$r" "explicit limit over zero failed"
    jt_assert_eq "$(jq '.result.structuredContent.data|length' <<<"$r")" "7" "explicit limit must override zero default"
    rollback_stop_server
    rollback_close_browser
    port=$((port+1))
  done
}

jt_register "RBK-001" "rollback" "Persistent/runtime rollback matrix" "Verify all four combinations of persistent MCP session and page runtime preserve snapshot, ref, click and search behavior while using the intended ref mechanism." "MCP credentials available; browser-perf small fixture." "Run persistent={0,1} × runtime={0,1} with snapshot limit 17." "All combinations work; runtime-on uses tokenized/no DOM refs and runtime-off uses legacy DOM refs." "mcp" rollback_matrix
jt_register "RBK-002" "rollback" "Same-document runtime on/off/on" "Verify toggling runtime feature flags between MCP server instances on the same document never leaves hybrid refs and always invalidates old namespaces." "MCP credentials; same browser document retained while MCP servers restart." "runtime on → off → on for persistent=0 and persistent=1." "Legacy/runtime states are coherent; stale refs from prior modes fail; re-enabled runtime uses fresh namespace." "mcp" rollback_transitions
jt_register "RBK-003" "rollback" "Snapshot-limit zero rollback" "Verify JELLY_SNAPSHOT_LIMIT=0 restores unlimited compatibility while explicit per-call limits still win." "Large fixture; MCP credentials." "runtime={0,1}, env snapshot limit 0, then explicit limit 7." "Default result contains >=1000 controls and explicit request contains exactly 7 in both modes." "mcp" rollback_limit_zero
