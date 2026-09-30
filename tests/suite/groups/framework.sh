#!/usr/bin/env bash

FRAMEWORK_URL="${JELLY_FRAMEWORK_TEST_URL:-https://todomvc.com/examples/react/dist/}"

framework_prepare() {
  jt_with_browser_cleanup
  local opened=false
  jt_close_browser
  for _ in {1..4}; do
    if "$JELLY_BIN_DIR/agent-open-browser" --headless "$FRAMEWORK_URL" >/dev/null 2>&1; then
      opened=true
      break
    fi
    jt_close_browser
    sleep 0.4
  done
  jt_assert_eq "$opened" "true" "React TodoMVC must open"
  local identity
  identity="$(jt_runtime_eval "({title:document.title,reactBundle:[...document.scripts].some(s=>(s.src||'').includes('/examples/react/dist/app.bundle.js'))})")"
  jt_assert_eq "$(jq -r '.title' <<<"$identity")" "TodoMVC: React" "framework page title must identify React TodoMVC"
  jt_assert_true "$(jq -r '.reactBundle' <<<"$identity")" "real React bundle must be loaded"
  jt_runtime_eval 'localStorage.clear(); true' >/dev/null
  "$JELLY_BIN_DIR/agent-navigate" "$FRAMEWORK_URL" >/dev/null
}

framework_wait_dirty() {
  local timeout_ms="${1:-2500}" elapsed=0
  while (( elapsed <= timeout_ms )); do
    [[ "$(jt_runtime_eval 'globalThis.__jellyRuntimeV1?.dirty===true')" == "true" ]] && return 0
    sleep 0.05
    elapsed=$((elapsed + 50))
  done
  jt_fail "runtime did not become dirty within ${timeout_ms}ms"
}

framework_wait_name() {
  local name="$1" timeout_ms="${2:-3000}" elapsed=0
  while (( elapsed <= timeout_ms )); do
    if jt_snapshot 100 0 | jq -e --arg n "$name" '.[]|select(.name==$n)' >/dev/null; then
      return 0
    fi
    sleep 0.05
    elapsed=$((elapsed + 50))
  done
  jt_fail "control '$name' did not appear within ${timeout_ms}ms"
}

framework_create() {
  local item="$1"
  local initial input_ref before after_fill epoch_after_fill epoch_after_create created item_ref elapsed
  initial="$(jt_snapshot 100 0)"
  input_ref="$(jq -r '.[]|select(.name=="New Todo Input")|.ref' <<<"$initial")"
  jt_assert_nonempty "$input_ref" "React Todo input ref missing"
  before="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.epoch')"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-fill" "$item" "$input_ref" >/dev/null
  framework_wait_dirty
  after_fill="$(jt_runtime_eval '({epoch:globalThis.__jellyRuntimeV1.epoch,dirty:globalThis.__jellyRuntimeV1.dirty})')"
  jt_assert_true "$(jq -r '.dirty' <<<"$after_fill")" "React controlled fill must dirty runtime"
  epoch_after_fill="$(jq -r '.epoch' <<<"$after_fill")"
  jt_assert_gt "$epoch_after_fill" "$before" "fill must advance invalidation epoch"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-press-key" Enter >/dev/null
  elapsed=0
  while (( elapsed <= 3000 )); do
    [[ "$(jt_runtime_eval "document.body.innerText.includes($(jq -Rn --arg x "$item" '$x'))")" == "true" ]] && break
    sleep 0.05
    elapsed=$((elapsed + 50))
  done
  (( elapsed <= 3000 )) || jt_fail "React-created todo did not appear in DOM"
  jt_assert_true "$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.dirty')" "React create rerender must keep runtime dirty"
  epoch_after_create="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.epoch')"
  jt_assert_eq "$epoch_after_create" "$epoch_after_fill" "additional React mutations while dirty must coalesce into the same invalidation epoch"
  created="$(jt_snapshot 100 0)"
  jt_assert_eq "$(jq -r --arg ref "$input_ref" '.[]|select(.name=="New Todo Input")|.ref==$ref' <<<"$created")" "true" "stable input must keep ref across React rerender"
  item_ref="$(jq -r --arg n "$item" '.[]|select(.name==$n)|.ref' <<<"$created")"
  jt_assert_nonempty "$item_ref" "created React todo ref missing"
  printf '%s\n%s\n' "$input_ref" "$item_ref"
}

framework_create_case() {
  framework_prepare
  local refs input_ref item_ref
  refs="$(framework_create 'Jelly framework create probe')"
  input_ref="$(sed -n '1p' <<<"$refs")"
  item_ref="$(sed -n '2p' <<<"$refs")"
  jt_assert_nonempty "$input_ref" "input ref missing after create workflow"
  jt_assert_nonempty "$item_ref" "item ref missing after create workflow"
  jt_assert_false "$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.dirty')" "snapshot after create must leave runtime clean"
}

framework_toggle_case() {
  framework_prepare
  local refs input_ref item_ref before state toggled
  refs="$(framework_create 'Jelly framework toggle probe')"
  input_ref="$(sed -n '1p' <<<"$refs")"
  item_ref="$(sed -n '2p' <<<"$refs")"
  before="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.epoch')"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-check" "$item_ref" >/dev/null
  framework_wait_dirty
  state="$(jt_runtime_eval '({epoch:globalThis.__jellyRuntimeV1.epoch,dirty:globalThis.__jellyRuntimeV1.dirty})')"
  jt_assert_gt "$(jq -r '.epoch' <<<"$state")" "$before" "React toggle must advance epoch"
  toggled="$(jt_snapshot 100 0)"
  jt_assert_eq "$(jq -r '.[]|select(.name=="Jelly framework toggle probe")|.checked' <<<"$toggled")" "true" "React-controlled checkbox must be observed checked"
  jt_assert_eq "$(jq -r '.[]|select(.name=="Jelly framework toggle probe")|.ref' <<<"$toggled")" "$item_ref" "React reused node must keep item ref"
  jt_assert_eq "$(jq -r --arg ref "$input_ref" '.[]|select(.name=="New Todo Input")|.ref==$ref' <<<"$toggled")" "true" "stable input ref must survive toggle rerender"
}

framework_remove_case() {
  framework_prepare
  local refs input_ref item_ref toggled clear_ref before cleared metrics
  refs="$(framework_create 'Jelly framework removal probe')"
  input_ref="$(sed -n '1p' <<<"$refs")"
  item_ref="$(sed -n '2p' <<<"$refs")"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-check" "$item_ref" >/dev/null
  framework_wait_dirty
  toggled="$(jt_snapshot 100 0)"
  clear_ref="$(jq -r '.[]|select(.name=="Clear completed")|.ref' <<<"$toggled")"
  jt_assert_nonempty "$clear_ref" "Clear completed control missing"
  before="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.epoch')"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-click" "$clear_ref" >/dev/null
  framework_wait_dirty
  jt_assert_gt "$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.epoch')" "$before" "React removal must advance epoch"
  cleared="$(jt_snapshot 100 0)"
  jt_assert_eq "$(jq '[.[]|select(.name=="Jelly framework removal probe")]|length' <<<"$cleared")" "0" "removed React item must disappear"
  jt_assert_eq "$(jq -r --arg ref "$input_ref" '.[]|select(.name=="New Todo Input")|.ref==$ref' <<<"$cleared")" "true" "stable input ref must survive removal"
  jt_expect_failure "removed React item ref must be stale" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" "$item_ref"
  metrics="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.lastRebuildTimings')"
  jq -e '.total_ms >= 0 and .candidates > 0 and .selected > 0 and .roots >= 1' >/dev/null <<<"$metrics" || jt_fail "rebuild metrics must be populated after reconciliation"
}

jt_register "FWK-001" "framework" "React controlled create" "Verify a real React controlled-input update and create rerender invalidate the runtime while stable nodes keep their refs." "Network access to React TodoMVC; localStorage can be cleared." "Fill New Todo Input and press Enter." "Runtime invalidates, item appears, input ref remains stable, and rebuilt runtime returns clean." "network" framework_create_case
jt_register "FWK-002" "framework" "React toggle reconciliation" "Verify React checkbox reconciliation updates live checked state without churning reused DOM-node refs." "Fresh React TodoMVC with one Jelly-created item." "Check the created item through Jelly." "Epoch advances, checked becomes true, item/input refs remain stable." "network" framework_toggle_case
jt_register "FWK-003" "framework" "React removal and stale refs" "Verify removing a React-controlled item invalidates the runtime, preserves unrelated refs, and rejects the removed ref." "Fresh React TodoMVC with a completed Jelly-created item." "Activate Clear completed." "Removed item disappears, stable input ref survives, removed item ref is stale, and rebuild metrics are valid." "network" framework_remove_case
