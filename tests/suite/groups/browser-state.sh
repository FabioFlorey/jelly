#!/usr/bin/env bash

BST_SERVER_PIDS=()
BST_PORT_FILES=()
BST_SERVER_LOGS=()
BST_PORTS=()
BST_DEST_DIR=""

browser_state_server_start() {
  local port_file log_file pid port
  port_file="$(mktemp)"
  log_file="/tmp/jelly-browser-state-server-$$-${#BST_SERVER_PIDS[@]}.log"
  [[ -x "$BIN_DIR/jelly-fixture-server" ]] || cargo build --locked --quiet --bin jelly-fixture-server
  "$BIN_DIR/jelly-fixture-server" "$port_file" >"$log_file" 2>&1 &
  pid=$!
  for _ in {1..80}; do
    if [[ -s "$port_file" ]]; then
      port="$(cat "$port_file")"
      if curl -fsS "http://127.0.0.1:$port/" >/dev/null 2>&1; then
        BST_SERVER_PIDS+=("$pid")
        BST_PORT_FILES+=("$port_file")
        BST_SERVER_LOGS+=("$log_file")
        BST_PORTS+=("$port")
        return 0
      fi
    fi
    sleep 0.05
  done
  cat "$log_file" >&2 || true
  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
  rm -f "$port_file" "$log_file"
  jt_fail "browser-state fixture server did not become ready"
}

browser_state_cleanup() {
  "$BIN_DIR/agent-close-browser" >/dev/null 2>&1 || true
  jt_wait_browser_service_gone >/dev/null 2>&1 || true
  local pid file
  for pid in "${BST_SERVER_PIDS[@]}"; do
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" >/dev/null 2>&1 || true
  done
  for file in "${BST_PORT_FILES[@]}" "${BST_SERVER_LOGS[@]}"; do
    [[ -z "$file" ]] || rm -f "$file"
  done
  [[ -z "$BST_DEST_DIR" ]] || rm -rf "$BST_DEST_DIR"
}

browser_state_ids() {
  "$BIN_DIR/agent-download" list | jq -c '[.downloads[].id]'
}

browser_state_new_download_id() {
  local before="$1" id
  for _ in {1..120}; do
    id="$("$BIN_DIR/agent-download" list | jq -r --argjson before "$before" '
      [.downloads[] | select((.id as $id | ($before | index($id))) == null)][0].id // empty
    ')"
    if [[ -n "$id" ]]; then
      printf '%s\n' "$id"
      return 0
    fi
    sleep 0.05
  done
  return 1
}

browser_state_wait_terminal() {
  local id="$1" state
  for _ in {1..120}; do
    state="$("$BIN_DIR/agent-download" status "$id" | jq -r '.state')"
    if [[ "$state" != "in_progress" ]]; then
      printf '%s\n' "$state"
      return 0
    fi
    sleep 0.05
  done
  return 1
}

bst_storage_cookie_api() {
  trap browser_state_cleanup EXIT
  browser_state_server_start
  browser_state_server_start
  local origin1="http://127.0.0.1:${BST_PORTS[0]}"
  local origin2="http://127.0.0.1:${BST_PORTS[1]}"
  local secret="bst-secret-$$-$RANDOM"
  local log="$CONFIG_RUNTIME_ROOT/logs/actions.jsonl" log_size=0 new_log
  [[ ! -f "$log" ]] || log_size="$(stat -c '%s' "$log")"

  jt_open_url "$origin1/"
  "$BIN_DIR/agent-storage-set" local alpha "$secret" >/dev/null
  "$BIN_DIR/agent-storage-set" session beta "session-value" >/dev/null
  jt_json_eq "$("$BIN_DIR/agent-storage-get" local alpha)" '.value' "$secret" "localStorage get must return the stored value"
  jt_assert_true "$("$BIN_DIR/agent-storage-list" local | jq -r --arg value "$secret" 'any(.entries[]; .[0]=="alpha" and .[1]==$value)')" "localStorage list must include the stored value"
  jt_json_eq "$("$BIN_DIR/agent-storage-get" session beta)" '.value' "session-value" "sessionStorage get must return the stored value"

  "$BIN_DIR/agent-navigate" "$origin2/" >/dev/null
  jt_assert_false "$("$BIN_DIR/agent-storage-get" local alpha | jq -r '.found')" "localStorage must be isolated by origin"
  jt_assert_false "$("$BIN_DIR/agent-storage-get" session beta | jq -r '.found')" "sessionStorage must be isolated by origin"

  "$BIN_DIR/agent-navigate" "$origin1/" >/dev/null
  jt_json_eq "$("$BIN_DIR/agent-storage-get" local alpha)" '.value' "$secret" "localStorage must survive same-tab cross-origin navigation"
  jt_json_eq "$("$BIN_DIR/agent-storage-get" session beta)" '.value' "session-value" "sessionStorage must remain scoped to its origin in the same tab"
  "$BIN_DIR/agent-storage-remove" local alpha >/dev/null
  jt_assert_false "$("$BIN_DIR/agent-storage-get" local alpha | jq -r '.found')" "storage-remove must remove the selected key"
  "$BIN_DIR/agent-storage-clear" session >/dev/null
  jt_assert_false "$("$BIN_DIR/agent-storage-get" session beta | jq -r '.found')" "storage-clear must clear only the selected storage area"

  local cookie_url="https://jelly-browser-state.invalid/" cookie
  cookie="$(jq -cn --arg value "$secret" --arg url "$cookie_url" '{name:"jelly_http_only",value:$value,url:$url,httpOnly:true,secure:true,sameSite:"Lax"}')"
  "$BIN_DIR/agent-set-cookie" "$cookie" >/dev/null
  jt_assert_true "$("$BIN_DIR/agent-cookies" "$cookie_url" | jq -r --arg value "$secret" 'any(.cookies[]; .name=="jelly_http_only" and .value==$value and .httpOnly==true)')" "cookies must expose the explicitly scoped HttpOnly cookie"
  "$BIN_DIR/agent-delete-cookie" "$(jq -cn --arg url "$cookie_url" '{name:"jelly_http_only",url:$url}')" >/dev/null
  jt_assert_false "$("$BIN_DIR/agent-cookies" "$cookie_url" | jq -r 'any(.cookies[]; .name=="jelly_http_only")')" "delete-cookie must remove the selected cookie"

  "$BIN_DIR/agent-set-cookie" "$(jq -cn --arg url "$cookie_url" '{name:"jelly_clear_a",value:"a",url:$url,secure:true}')" >/dev/null
  "$BIN_DIR/agent-set-cookie" "$(jq -cn --arg url "$cookie_url" '{name:"jelly_clear_b",value:"b",url:$url,secure:true}')" >/dev/null
  jt_assert_ge "$("$BIN_DIR/agent-clear-cookies" "$cookie_url" | jq -r '.deleted')" "2" "clear-cookies must remove cookies applicable to the supplied URL"
  jt_assert_false "$("$BIN_DIR/agent-cookies" "$cookie_url" | jq -r 'any(.cookies[]; (.name=="jelly_clear_a" or .name=="jelly_clear_b"))')" "clear-cookies must remove the scoped test cookies"

  if [[ -f "$log" ]]; then
    new_log="$(tail -c "+$((log_size + 1))" "$log")"
    [[ "$new_log" != *"$secret"* ]] || jt_fail "cookie/storage secret must not be persisted in primitive trace arguments"
  fi
}

bst_download_completion_and_collision() {
  trap browser_state_cleanup EXIT
  browser_state_server_start
  local origin="http://127.0.0.1:${BST_PORTS[0]}" before id result legacy baseline
  local fail_log="/tmp/jelly-bst-download-fail-$$.log"
  BST_DEST_DIR="$(mktemp -d)"
  printf 'existing' > "$BST_DEST_DIR/fixture.txt"

  jt_open_url "$origin/"
  before="$(browser_state_ids)"
  "$BIN_DIR/agent-click" 'css:#fast' >/dev/null
  id="$(browser_state_new_download_id "$before")" || jt_fail "download tracker must publish a GUID after downloadWillBegin"

  set +e
  "$BIN_DIR/agent-download" wait "$id" 10 "$BST_DEST_DIR" fail >"$fail_log" 2>&1
  local fail_code=$?
  set -e
  [[ "$fail_code" -ne 0 ]] || jt_fail "collision=fail must reject an existing destination"
  jt_assert_eq "$(cat "$BST_DEST_DIR/fixture.txt")" "existing" "collision=fail must not overwrite the existing destination"

  result="$("$BIN_DIR/agent-download" wait "$id" 10 "$BST_DEST_DIR" uniquify)"
  jt_json_eq "$result" '.state' "completed" "download wait must observe CDP completion"
  jt_json_eq "$result" '.suggested_filename' "fixture.txt" "download must preserve Chromium's suggested filename"
  jt_assert_ge "$(jq -r '.received_bytes' <<<"$result")" "1" "download progress must expose received bytes"
  jt_assert_ge "$(jq -r '.total_bytes' <<<"$result")" "1" "download progress must expose total bytes"
  jt_json_eq "$result" '.collision_policy' "uniquify" "download must retain the applied collision policy"
  jt_json_eq "$result" '.artifact.source.download_id' "$id" "registered artifact must retain its download GUID"
  jt_json_eq "$result" '.artifact.source.suggested_filename' "fixture.txt" "registered artifact must retain suggested filename provenance"
  jt_assert_eq "$(basename "$(jq -r '.materialized_path' <<<"$result")")" "fixture (1).txt" "uniquify must select a non-colliding destination"
  jt_assert_eq "$(cat "$(jq -r '.materialized_path' <<<"$result")")" "jelly-download-fixture" "materialized download must contain the downloaded bytes"

  baseline="$(date +%s%3N)"
  "$BIN_DIR/agent-click" 'css:#fast' >/dev/null
  legacy="$("$BIN_DIR/agent-wait-download" "$baseline" 10 fixture.txt)"
  jt_assert_nonempty "$(jq -r '.source.download_id // empty' <<<"$legacy")" "legacy wait-download must prefer first-class lifecycle evidence when available"
  rm -f "$fail_log"
}

bst_download_cancel_and_browser_stop() {
  trap browser_state_cleanup EXIT
  browser_state_server_start
  local origin="http://127.0.0.1:${BST_PORTS[0]}" before id status state second

  jt_open_url "$origin/"
  before="$(browser_state_ids)"
  "$BIN_DIR/agent-click" 'css:#slow' >/dev/null
  id="$(browser_state_new_download_id "$before")" || jt_fail "slow download must publish a lifecycle GUID"
  status="$("$BIN_DIR/agent-download" status "$id")"
  jt_json_eq "$status" '.state' "in_progress" "new slow download must be cancellable before terminal completion"
  "$BIN_DIR/agent-download" cancel "$id" >/dev/null
  state="$(browser_state_wait_terminal "$id")" || jt_fail "canceled download must reach a terminal state"
  jt_assert_eq "$state" "canceled" "cancel must produce canceled lifecycle state"
  jt_json_eq "$("$BIN_DIR/agent-download" status "$id")" '.failure_reason' "canceled_by_user" "explicit cancellation must retain its failure reason"

  before="$(browser_state_ids)"
  "$BIN_DIR/agent-click" 'css:#slow' >/dev/null
  second="$(browser_state_new_download_id "$before")" || jt_fail "second slow download must publish a lifecycle GUID"
  jt_close_browser
  status="$("$BIN_DIR/agent-download" status "$second")"
  [[ "$(jq -r '.state' <<<"$status")" != "in_progress" ]] || jt_fail "graceful browser stop must not leave an in-progress download record"
  local stopped_state stopped_reason
  stopped_state="$(jq -r '.state' <<<"$status")"
  stopped_reason="$(jq -r '.failure_reason // empty' <<<"$status")"
  jt_assert_nonempty "$stopped_reason" "browser-stopped download must retain a failure reason"

  jt_open_url "$origin/"
  status="$("$BIN_DIR/agent-download" status "$second")"
  jt_json_eq "$status" '.state' "$stopped_state" "download terminal state must persist across browser restart"
  jt_json_eq "$status" '.failure_reason' "$stopped_reason" "download failure reason must persist across browser restart"
}

bst_download_unclean_browser_exit() {
  trap browser_state_cleanup EXIT
  browser_state_server_start
  local origin="http://127.0.0.1:${BST_PORTS[0]}" before id status pid

  jt_open_url "$origin/"
  before="$(browser_state_ids)"
  "$BIN_DIR/agent-click" 'css:#slow' >/dev/null
  id="$(browser_state_new_download_id "$before")" || jt_fail "slow download must publish a lifecycle GUID"
  status="$("$BIN_DIR/agent-download" status "$id")"
  jt_json_eq "$status" '.state' "in_progress" "slow download must still be active before forced browser exit"

  pid="$(cat "$CONFIG_RUNTIME_ROOT/state/browser.pid")"
  kill -KILL "$pid"
  jt_wait_browser_service_gone

  status="$("$BIN_DIR/agent-download" status "$id")"
  jt_json_eq "$status" '.state' "interrupted" "status must reconcile an orphaned in-progress download after browser process death"
  jt_json_eq "$status" '.failure_reason' "browser_process_exited_before_completion" "unclean browser death must retain a deterministic failure reason"
}

jt_register "BST-001" "browser-state" "Origin storage and cookie API" "Verify first-class cookie plus localStorage/sessionStorage operations without page JavaScript or raw CDP." "Two deterministic local HTTP origins can be served and Chromium can open the fixture." "Set/read/list/remove/clear DOM storage across origins; set/read/delete/clear scoped cookies including HttpOnly; inspect new trace records." "Storage stays origin-scoped, cookie operations work through semantic CDP, and secret values are absent from primitive trace arguments." "browser" bst_storage_cookie_api
jt_register "BST-002" "browser-state" "Download lifecycle and destination policy" "Verify CDP-backed download identity, progress, completion, suggested filename, artifact provenance, destination selection, collision policy, and legacy wait compatibility." "Deterministic local download fixture and writable temporary destination are available." "Trigger a fast download, exercise fail/uniquify destination policies, then trigger a second download through legacy wait-download." "GUID/progress/completion metadata are present, fail never overwrites, uniquify materializes safely, artifacts retain provenance, and legacy wait uses lifecycle evidence." "browser" bst_download_completion_and_collision
jt_register "BST-003" "browser-state" "Download cancellation and browser-stop state" "Verify explicit cancellation and graceful browser shutdown produce terminal first-class download records with failure reasons." "Deterministic slow local download endpoint is available." "Cancel one in-progress download, then stop Chromium during a second in-progress download." "Explicit cancel becomes canceled_by_user and browser stop never leaves a stale in_progress record." "browser" bst_download_cancel_and_browser_stop
jt_register "BST-004" "browser-state" "Unclean browser-exit download reconciliation" "Verify a browser process crash cannot leave first-class download state stuck in_progress indefinitely." "Deterministic slow local download endpoint and transient jelly-browser service are available." "Start a slow download, SIGKILL the browser launcher service process, then query the download record without restarting Chromium." "The orphaned record is reconciled to interrupted with browser_process_exited_before_completion." "browser" bst_download_unclean_browser_exit
