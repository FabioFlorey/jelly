#!/usr/bin/env bash

session_load_env() {
  # Shared config.sh already loads the data-only .env and preserves overrides.
  : "${JELLY_MCP_TOKEN:?JELLY_MCP_TOKEN is required for MCP session tests}"
  : "${JELLY_BOOTSTRAP_SECRET:?JELLY_BOOTSTRAP_SECRET is required for MCP session tests}"
}

session_start_mcp() {
  local port="$1" perf="${2:-0}"
  SESSION_MCP_PORT="$port"
  SESSION_MCP_LOG="/tmp/jelly-suite-session-${port}.log"
  jt_config_set mcp surface large-surface
  jt_config_set mcp persistent_session true
  jt_config_set diagnostics perf_log "$([[ "$perf" == 1 ]] && echo true || echo false)"
  JELLY_MCP_ADDR="127.0.0.1:${port}" \
  JELLY_PUBLIC_URL="http://127.0.0.1:${port}" \
    "$BIN_DIR/jelly-mcp" >"$SESSION_MCP_LOG" 2>&1 &
  SESSION_MCP_PID=$!
  for _ in {1..80}; do
    curl -fsS "http://127.0.0.1:${port}/health" >/dev/null 2>&1 && return 0
    sleep 0.1
  done
  cat "$SESSION_MCP_LOG" >&2 || true
  jt_fail "MCP session test server did not become ready on port $port"
}

session_cleanup() {
  if [[ -n "${SESSION_MCP_PID:-}" ]]; then
    kill "$SESSION_MCP_PID" >/dev/null 2>&1 || true
    wait "$SESSION_MCP_PID" >/dev/null 2>&1 || true
  fi
  jt_close_browser
}

session_call() {
  local id="$1" name="$2" args="$3"
  local payload
  payload="$(jq -cn --argjson id "$id" --arg name "$name" --argjson arguments "$args" '{jsonrpc:"2.0",id:$id,method:"tools/call",params:{name:$name,arguments:$arguments}}')"
  curl -fsS \
    -H "Authorization: Bearer $JELLY_MCP_TOKEN" \
    -H 'Content-Type: application/json' \
    -X POST "http://127.0.0.1:${SESSION_MCP_PORT}/mcp" \
    --data "$payload"
}

session_expect_ok() {
  jq -e '.result.structuredContent.ok == true' >/dev/null <<<"$1" || jt_fail "$2"
}

session_expect_browser_unavailable() {
  jq -e '.result.structuredContent.ok == false and .result.structuredContent.error.kind == "browser_unavailable" and .result.structuredContent.error.retryable == true' >/dev/null <<<"$1" || jt_fail "$2"
}

session_prepare() {
  session_load_env
  trap session_cleanup EXIT
  jt_open_fixture browser-perf.html
}

session_reuse() {
  session_prepare
  local perf_log="$CONFIG_RUNTIME_ROOT/logs/perf.jsonl"
  rm -f "$perf_log"
  session_start_mcp 18789 1
  local fixture="file://${REPO_ROOT}/tests/fixtures/browser-perf.html"
  local r
  r="$(session_call 1 snapshot-interactive '{"limit":5}')"; session_expect_ok "$r" "initial snapshot failed"
  r="$(session_call 2 navigate "$(jq -cn --arg url "$fixture" '{url:$url}')")"; session_expect_ok "$r" "navigate failed"
  r="$(session_call 3 snapshot-interactive '{"limit":5}')"; session_expect_ok "$r" "snapshot after navigation failed"
  r="$(session_call 4 open-in-new-tab '{"target":"css:a"}')"; session_expect_ok "$r" "open-in-new-tab failed"
  r="$(session_call 5 snapshot-interactive '{"limit":5}')"; session_expect_ok "$r" "snapshot on second target failed"
  r="$(session_call 6 close-tab '{}')"; session_expect_ok "$r" "close-tab failed"
  r="$(session_call 7 snapshot-interactive '{"limit":5}')"; session_expect_ok "$r" "snapshot after close-tab failed"
  local connects
  connects="$(jq -s '[.[] | select(.event == "browser.connect")] | length' "$perf_log")"
  jt_assert_eq "$connects" "1" "healthy persistent sequence must use one browser connection"
}

session_external_target() {
  session_prepare
  session_start_mcp 18790 0
  local r
  r="$(session_call 1 snapshot-interactive '{"limit":5}')"; session_expect_ok "$r" "initial snapshot failed"
  r="$(session_call 2 open-in-new-tab '{"target":"css:a"}')"; session_expect_ok "$r" "open-in-new-tab failed"
  r="$(session_call 3 evaluate-js '{"expression":"document.title = \"MCP secondary tab\""}')"; session_expect_ok "$r" "marking second tab failed"
  "$BIN_DIR/agent-switch-tab" "Jelly browser performance fixture" >/dev/null
  r="$(session_call 4 evaluate-js '{"expression":"document.title"}')"; session_expect_ok "$r" "evaluate after external switch failed"
  jt_assert_eq "$(jq -r '.result.structuredContent.data' <<<"$r")" "Jelly browser performance fixture" "persistent session must follow externally selected target"
}

session_stale_target() {
  session_prepare
  session_start_mcp 18791 0
  local r stale repaired
  r="$(session_call 1 snapshot-interactive '{"limit":5}')"; session_expect_ok "$r" "initial snapshot failed"
  r="$(session_call 2 open-in-new-tab '{"target":"css:a"}')"; session_expect_ok "$r" "open-in-new-tab failed"
  stale="$(cat "$CONFIG_RUNTIME_ROOT/state/active_target_id")"
  "$BIN_DIR/agent-close-tab" >/dev/null
  printf '%s' "$stale" > "$CONFIG_RUNTIME_ROOT/state/active_target_id"
  r="$(session_call 3 snapshot-interactive '{"limit":5}')"
  session_expect_browser_unavailable "$r" "stale target must fail once as retryable browser_unavailable"
  r="$(session_call 4 snapshot-interactive '{"limit":5}')"; session_expect_ok "$r" "second call must reconnect through fallback"
  repaired="$(cat "$CONFIG_RUNTIME_ROOT/state/active_target_id")"
  jt_assert_ne "$repaired" "$stale" "reconnect must repair shared active target state"
}

session_dead_browser() {
  session_prepare
  session_start_mcp 18792 0
  local r fixture="file://${REPO_ROOT}/tests/fixtures/browser-perf.html"
  r="$(session_call 1 snapshot-interactive '{"limit":5}')"; session_expect_ok "$r" "initial snapshot failed"
  "$BIN_DIR/agent-close-browser" >/dev/null
  r="$(session_call 2 snapshot-interactive '{"limit":5}')"
  session_expect_browser_unavailable "$r" "dead browser must invalidate cached MCP session"
  local opened=false
  for _ in {1..8}; do
    if "$BIN_DIR/agent-open-browser" --headless "$fixture" >/dev/null 2>&1 && "$BIN_DIR/agent-evaluate-js" 'document.readyState' >/dev/null 2>&1; then
      opened=true
      break
    fi
    jt_close_browser
    sleep 0.25
  done
  jt_assert_eq "$opened" "true" "browser service must become ready after deliberate shutdown"
  r="$(session_call 3 snapshot-interactive '{"limit":5}')"; session_expect_ok "$r" "first MCP call after browser reopen must reconnect"
}

jt_register "SES-001" "session" "Persistent CDP reuse" "Verify a healthy MCP sequence reuses exactly one CDP BrowserSession across navigation and tab operations." "Headless browser fixture; MCP token and bootstrap secret available." "snapshot → navigate → snapshot → open tab → snapshot → close tab → snapshot" "Every tool call succeeds and perf log contains exactly one browser.connect event." "mcp" session_reuse
jt_register "SES-002" "session" "External active-target synchronization" "Verify a cached persistent session follows an active-tab change performed outside MCP." "Persistent MCP session attached to browser with two tabs." "Switch active tab with CLI, then evaluate document.title through MCP." "MCP evaluates against the externally selected tab without reconnecting incorrectly." "mcp" session_external_target
jt_register "SES-003" "session" "Stale target recovery" "Verify a genuinely stale active target fails once, invalidates the cache, then reconnects using the primary-page fallback." "Persistent MCP session and writable active_target_id state file." "Close active tab externally, restore stale target ID, issue two snapshots." "First call returns retryable browser_unavailable; second succeeds and active_target_id is repaired." "mcp" session_stale_target
jt_register "SES-004" "session" "Dead browser recovery" "Verify killing the browser invalidates the persistent MCP session and a later browser restart reconnects cleanly." "Persistent MCP session attached to a live browser." "Close browser, call snapshot, restart browser, call snapshot again." "Dead-browser call is retryable browser_unavailable; reopened browser becomes ready and next call succeeds." "mcp" session_dead_browser
