#!/usr/bin/env bash

ranking_prepare_large() {
  jt_with_browser_cleanup
  jt_open_fixture browser-perf.html
  jt_runtime_eval "window.__jellyBench.setCase('large')" >/dev/null
}

ranking_join_refs() {
  jq -r '.[].ref' | paste -sd, -
}

ranking_validation() {
  ranking_prepare_large
  jt_expect_failure "snapshot must reject non-numeric limit" "$BIN_DIR/agent-snapshot-interactive" nope
  jt_expect_failure "snapshot must reject negative offset" "$BIN_DIR/agent-snapshot-interactive" 10 -1
  jt_config_set page snapshot_limit nope
  jt_expect_failure "snapshot must reject malformed configured limit" "$BIN_DIR/agent-snapshot-interactive"
  jt_config_set page snapshot_limit 0
  jt_expect_failure "search must reject zero limit" "$BIN_DIR/agent-find-interactive" large 0
  jt_expect_failure "search must reject negative offset" "$BIN_DIR/agent-find-interactive" large 10 -1
  jt_assert_eq "$("$BIN_DIR/agent-snapshot-interactive" 999999 | jq 'length')" "1000" "snapshot limit must clamp to 1000"
  jt_assert_eq "$("$BIN_DIR/agent-find-interactive" large 999999 | jq 'length')" "100" "semantic search limit must clamp to 100"
}

ranking_snapshot_pagination() {
  ranking_prepare_large
  local mode label full p1 p2 combined first50 configured_limited explicit_limited beyond
  for mode in 1 0; do
    label="$([[ "$mode" == 1 ]] && echo runtime || echo legacy)"
    jt_config_set page runtime "$([[ "$mode" == 1 ]] && echo true || echo false)"
    jt_config_set page snapshot_limit 0
    full="$("$BIN_DIR/agent-snapshot-interactive")"
    jt_assert_ge "$(jq 'length' <<<"$full")" "1000" "$label unlimited snapshot must remain complete"
    p1="$("$BIN_DIR/agent-snapshot-interactive" 25 0)"
    p2="$("$BIN_DIR/agent-snapshot-interactive" 25 25)"
    jt_assert_eq "$(jq 'length' <<<"$p1")" "25" "$label first page size"
    jt_assert_eq "$(jq 'length' <<<"$p2")" "25" "$label second page size"
    combined="$(printf '%s\n%s\n' "$p1" "$p2" | jq -s add | ranking_join_refs)"
    first50="$(jq '.[0:50]' <<<"$full" | ranking_join_refs)"
    jt_assert_eq "$combined" "$first50" "$label snapshot pagination must preserve deterministic order"
    jt_config_set page snapshot_limit 17
    configured_limited="$("$BIN_DIR/agent-snapshot-interactive")"
    jt_assert_eq "$(jq 'length' <<<"$configured_limited")" "17" "$label configured snapshot limit"
    explicit_limited="$("$BIN_DIR/agent-snapshot-interactive" 9)"
    jt_assert_eq "$(jq 'length' <<<"$explicit_limited")" "9" "$label explicit limit must override configured limit"
    jt_config_set page snapshot_limit 0
    beyond="$("$BIN_DIR/agent-snapshot-interactive" 25 999999)"
    jt_assert_eq "$(jq 'length' <<<"$beyond")" "0" "$label offset beyond result set must be empty"
  done
}

ranking_search_pagination() {
  ranking_prepare_large
  local mode label s1 s2 combined s20 overlap
  for mode in 1 0; do
    label="$([[ "$mode" == 1 ]] && echo runtime || echo legacy)"
    jt_config_set page runtime "$([[ "$mode" == 1 ]] && echo true || echo false)"
    s1="$("$BIN_DIR/agent-find-interactive" large 10 0)"
    s2="$("$BIN_DIR/agent-find-interactive" large 10 10)"
    jt_assert_eq "$(jq 'length' <<<"$s1")" "10" "$label first search page size"
    jt_assert_eq "$(jq 'length' <<<"$s2")" "10" "$label second search page size"
    combined="$(printf '%s\n%s\n' "$s1" "$s2" | jq -s add | ranking_join_refs)"
    s20="$("$BIN_DIR/agent-find-interactive" large 20 0 | ranking_join_refs)"
    jt_assert_eq "$combined" "$s20" "$label search pagination must preserve rank order"
    overlap="$(comm -12 <(jq -r '.[].ref' <<<"$s1" | sort) <(jq -r '.[].ref' <<<"$s2" | sort) | wc -l | tr -d ' ')"
    jt_assert_eq "$overlap" "0" "$label search pages must not overlap"
  done
}

ranking_parity() {
  jt_with_browser_cleanup
  jt_open_fixture semantic-targets.html
  local mode label priority duplicate viewport
  for mode in 1 0; do
    label="$([[ "$mode" == 1 ]] && echo runtime || echo legacy)"
    jt_config_set page runtime "$([[ "$mode" == 1 ]] && echo true || echo false)"
    priority="$("$BIN_DIR/agent-find-interactive" 'Priority action' 5)"
    jt_assert_false "$(jq -r '.[0].disabled' <<<"$priority")" "$label enabled duplicate must rank first"
    jt_assert_false "$(jq -r '.[0].in_viewport // false' <<<"$priority")" "$label enabled state must outrank viewport"
    duplicate="$("$BIN_DIR/agent-find-interactive" 'Duplicate action' 5)"
    jt_assert_false "$(jq -r '.[0].disabled' <<<"$duplicate")" "$label duplicate must prefer enabled control"
    viewport="$("$BIN_DIR/agent-find-interactive" 'Viewport action' 5)"
    if [[ "$mode" == 1 ]]; then
      jt_assert_true "$(jq -r '.[0].in_viewport' <<<"$viewport")" "runtime equal-actionability tie must prefer onscreen control"
    else
      # legacy output exposes in_viewport from find-interactive as well
      jt_assert_true "$(jq -r '.[0].in_viewport' <<<"$viewport")" "legacy equal-actionability tie must prefer onscreen control"
    fi
  done
}

jt_register "RANK-001" "ranking" "Limit validation and clamping" "Verify snapshot/search pagination arguments reject malformed values and cap oversized limits safely." "Large local browser fixture." "Invalid, negative, zero and oversized limit/offset arguments." "Invalid values fail; snapshot clamps to 1000 and search clamps to 100." "browser" ranking_validation
jt_register "RANK-002" "ranking" "Snapshot pagination" "Verify bounded snapshot paging is deterministic and equivalent in runtime and legacy modes." "Large fixture with >1000 interactive controls." "Unlimited snapshot, pages 0/25, configured/explicit limits, beyond-end offset in both modes." "Pages have requested sizes/order, explicit overrides configured default, beyond-end is empty, unlimited stays complete." "browser" ranking_snapshot_pagination
jt_register "RANK-003" "ranking" "Semantic-search pagination" "Verify semantic-search pages preserve ranking order and never overlap in runtime and legacy modes." "Large fixture containing many matching interactive names." "Search 'large' with offsets 0 and 10, compare with one 20-result page." "Two pages concatenate exactly to first 20 ranked results and share no refs." "browser" ranking_search_pagination
jt_register "RANK-004" "ranking" "Runtime/legacy ranking parity" "Verify actionability/viewport precedence is consistent between optimized runtime and rollback implementation." "semantic-targets.html controlled duplicate fixture." "Search Priority action, Duplicate action and Viewport action with runtime on/off." "Both modes prefer enabled controls and use viewport only after actionability." "browser" ranking_parity
