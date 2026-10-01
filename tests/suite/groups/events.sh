#!/usr/bin/env bash

events_probe_build() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo build --quiet --locked \
    --manifest-path tests/suite/support/event-probe/Cargo.toml
}

events_prepare() {
  trap jt_close_browser EXIT
  events_probe_build
  jt_open_fixture browser-perf.html
}

events_idle_exact() {
  events_prepare
  local result
  result="$("$JELLY_BIN_DIR/jelly-event-probe" 64 100)"

  jt_assert_eq "$(jq -r '.before_idle.retained_count' <<<"$result")" "0"     "event ring must be empty immediately before the idle period"
  jt_assert_eq "$(jq -r '.after_idle_before_poll.retained_count' <<<"$result")" "0"     "idle websocket traffic must remain unread until a later pump"
  jt_assert_eq "$(jq -r '.poll.events' <<<"$result")" "64"     "barrier poll must recover every moderate idle event"
  jt_assert_eq "$(jq -r '.poll.unique_events' <<<"$result")" "64"     "barrier poll must not duplicate moderate idle events"
  jt_assert_eq "$(jq -r '.poll.first_index' <<<"$result")" "0"     "moderate idle recovery must begin with the first scheduled event"
  jt_assert_eq "$(jq -r '.poll.last_index' <<<"$result")" "63"     "moderate idle recovery must end with the last scheduled event"
  jt_assert_true "$(jq -r '.poll.contiguous' <<<"$result")"     "moderate idle recovery must preserve contiguous event order"
  jt_assert_false "$(jq -r '.poll.cursor_lost' <<<"$result")"     "moderate idle recovery must not report cursor loss"
  jt_assert_eq "$(jq -r '.poll.dropped' <<<"$result")" "0"     "moderate idle recovery must not drop events"
  jt_assert_eq "$(jq -r '.poll.stream_resets' <<<"$result")" "0"     "moderate idle recovery must not reset the stream"
}

events_multitab_routing() {
  events_prepare
  local result
  result="$("$JELLY_BIN_DIR/jelly-event-probe" multitab)"

  jt_assert_true "$(jq -r '.sessions_distinct' <<<"$result")"     "main and tab-2 must have distinct auto-attached CDP sessions"
  jt_assert_eq "$(jq -r '.routed | length' <<<"$result")" "2"     "multi-tab probe must retain exactly two routed console events"
  jt_assert_eq "$(jq -r '.routed[0][0]' <<<"$result")" "main"     "first routed event must belong to main after sorting"
  jt_assert_eq "$(jq -r '.routed[0][1]' <<<"$result")" "from-main"     "main event payload must be preserved"
  jt_assert_eq "$(jq -r '.routed[1][0]' <<<"$result")" "tab-2"     "second routed event must belong to tab-2 after sorting"
  jt_assert_eq "$(jq -r '.routed[1][1]' <<<"$result")" "from-tab-2"     "tab-2 event payload must be preserved"
  jt_assert_eq "$(jq -r '.active_after_target_calls' <<<"$result")"     "$(jq -r '.active_before' <<<"$result")"     "direct logical-target CDP dispatch must not mutate the active tab"
  jt_assert_true "$(jq -r '.destroyed_routed' <<<"$result")"     "Target.targetDestroyed must retain the destroyed logical label"
  jt_assert_true "$(jq -r '.tab2_removed' <<<"$result")"     "destroyed tab-2 must be removed from the logical registry"
}

events_idle_overflow() {
  events_prepare
  local result
  result="$("$JELLY_BIN_DIR/jelly-event-probe" 1500 100)"

  jt_assert_eq "$(jq -r '.after_idle_before_poll.retained_count' <<<"$result")" "0"     "idle burst must remain unread before poll"
  jt_assert_eq "$(jq -r '.final_ring.max_count' <<<"$result")" "1024"     "probe assumes the default count-bounded event ring"
  jt_assert_eq "$(jq -r '.final_ring.retained_count' <<<"$result")" "1024"     "overflow must retain exactly the bounded ring capacity"
  jt_assert_eq "$(jq -r '.final_ring.dropped_total' <<<"$result")" "476"     "overflow must account for every event evicted by the 1024-entry bound"
  jt_assert_eq "$(jq -r '.poll.events' <<<"$result")" "1024"     "overflow poll must return every retained matching event"
  jt_assert_eq "$(jq -r '.poll.unique_events' <<<"$result")" "1024"     "overflow poll must not duplicate retained events"
  jt_assert_eq "$(jq -r '.poll.first_index' <<<"$result")" "476"     "overflow must retain the deterministic suffix after oldest eviction"
  jt_assert_eq "$(jq -r '.poll.last_index' <<<"$result")" "1499"     "overflow must retain the newest scheduled event"
  jt_assert_true "$(jq -r '.poll.contiguous' <<<"$result")"     "retained overflow suffix must remain contiguous"
  jt_assert_true "$(jq -r '.poll.cursor_lost' <<<"$result")"     "bounded overflow must explicitly report cursor loss"
  jt_assert_eq "$(jq -r '.poll.dropped' <<<"$result")" "476"     "poll loss delta must equal the known bounded overflow"
  jt_assert_eq "$(jq -r '.poll.stream_resets' <<<"$result")" "0"     "bounded overflow is not a stream reset"
}

jt_register "EVT-001" "events" "Idle event barrier completeness" "Verify target-scoped CDP notifications emitted while Jelly performs no websocket reads are recovered completely by the next browser-events poll barrier." "Headless browser-perf fixture; Runtime domain enabled by the probe; one persistent BrowserSession." "Schedule 64 console notifications after the scheduling request returns, sleep through the idle period without CDP reads, then poll browser-events." "Ring remains unchanged during idle; first poll recovers all 64 unique events in order with no drop, cursor loss, or reset." "browser" events_idle_exact
jt_register "EVT-002" "events" "Idle burst bounded-loss accounting" "Verify a burst larger than the event ring can remain buffered during Jelly idle and is drained without silent loss: only the configured ring eviction is reported." "Headless browser-perf fixture; default 1024-entry event ring; one persistent BrowserSession." "Schedule 1500 console notifications during idle, then drain via browser-events poll with pagination." "Exactly 1024 newest events remain as a contiguous suffix; 476 oldest events are explicitly reported dropped with cursor_lost=true and no stream reset." "browser" events_idle_overflow
jt_register "EVT-003" "events" "Multi-tab auto-attach routing" "Verify Jelly auto-attaches page targets, routes target-scoped notifications to logical labels, and removes destroyed targets without exposing CDP session IDs." "Headless browser-perf fixture; one persistent BrowserSession with page-only Target auto-attach enabled." "Create tab-2; enable Runtime on main/tab-2; emit one console event per tab through a target-scoped raw browser-call batch; poll; close tab-2." "main/tab-2 receive distinct sessions; events route to the correct logical target; targeted raw CDP does not change the active tab; targetDestroyed retains tab-2 attribution and removes it from the registry." "browser" events_multitab_routing
