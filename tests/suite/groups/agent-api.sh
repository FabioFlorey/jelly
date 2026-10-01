#!/usr/bin/env bash

agent_api_probe_build() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo build --quiet --locked \
    --manifest-path tests/suite/support/agent-api-probe/Cargo.toml
}

agent_api_probe() {
  local mode="$1"
  agent_api_probe_build
  "$JELLY_BIN_DIR/jelly-agent-api-probe" "$mode"
}

agent_api_browser_probe() {
  local mode="$1"
  trap jt_close_browser EXIT
  agent_api_probe_build
  jt_open_fixture browser-perf.html
  "$JELLY_BIN_DIR/jelly-agent-api-probe" "$mode"
}

api_001() { agent_api_probe small-surface-catalog; }
api_002() { agent_api_probe large-surface-catalog; }
api_003() { agent_api_probe system-parity; }
api_004() { agent_api_probe schema-capabilities; }
api_005() { agent_api_probe schema-search; }
api_006() { agent_api_probe schema-contract; }
api_007() { agent_api_probe raw-disabled; }
api_008() { agent_api_probe large-surface-raw; }
api_009() { agent_api_browser_probe semantic-read; }
api_010() { agent_api_browser_probe semantic-mutate-verify; }
api_011() { agent_api_browser_probe preflight-atomic; }
api_012() { agent_api_browser_probe failure-stop; }
api_013() { agent_api_browser_probe failure-continue; }
api_014() { agent_api_browser_probe stale-ref; }
api_015() { agent_api_browser_probe logical-target; }
api_016() { agent_api_browser_probe raw-target; }
api_017() { agent_api_browser_probe raw-browser; }
api_018() { agent_api_browser_probe mixed-batch; }
api_019() { agent_api_browser_probe events-lifecycle; }
api_020() { agent_api_browser_probe subscription-stale; }
api_021() { agent_api_probe default-small-surface; }
api_022() { agent_api_probe explicit-large-surface; }

jt_register "API-001" "agent-api" "Small-surface catalog browser facade" "Verify small-surface publication exposes exactly the browser facade builtins rather than individual semantic primitives." "Agent Tool Catalog can be constructed without a browser." "Build the small-surface catalog with raw CDP disabled and compare published names against primitive/system registries." "browser-schema, browser-call, and browser-events are present; every individual browser primitive is absent; system tools remain present." "quality" api_001
jt_register "API-002" "agent-api" "Large-surface catalog" "Verify the large-surface catalog continues to publish individual browser primitives and excludes small-surface facade builtins." "Agent Tool Catalog can be constructed without a browser." "Build the large-surface catalog and inspect browser entries." "Registered browser primitives remain published and browser-schema/browser-call/browser-events remain absent." "quality" api_002
jt_register "API-003" "agent-api" "System-tool parity across surfaces" "Verify small-surface browser rollout does not alter system-tool publication order or bindings." "Large-surface and small-surface Agent Tool Catalogs are constructible." "Project system bindings from both catalogs and compare with the canonical system registry." "Large-surface and small-surface expose the exact ordered system-tool registry." "quality" api_003
jt_register "API-004" "agent-api" "Small-surface capability discovery" "Verify browser-schema capabilities describes the complete semantic browser registry without system-tool leakage." "Internal browser primitive registry is valid." "Execute browser-schema action=capabilities." "Schema version is 1 and operation_count exactly matches the semantic primitive registry." "quality" api_004
jt_register "API-005" "agent-api" "Small-surface search determinism" "Verify browser-schema semantic search is deterministic and intent-ranked." "Internal semantic discovery index is valid." "Search twice for upload local file with the same limit." "Results are byte-equivalent across calls and upload is the top semantic operation." "quality" api_005
jt_register "API-006" "agent-api" "Named semantic schema contract" "Verify small-surface discovery returns strict named JSON contracts rather than CLI usage metadata." "snapshot-interactive has a valid named argument contract." "Load browser-schema schema for snapshot-interactive." "Returned operation has a strict object input schema and does not expose positional CLI usage." "quality" api_006
jt_register "API-007" "agent-api" "Raw CDP disabled boundary" "Verify raw method-form browser-call entries are neither published nor accepted when raw CDP is disabled." "Small-surface browser-call schema is available with RawCdpAccess disabled." "Inspect browser-call input schema and preflight a browser-scoped Target.getTargets method-form call." "No method field is published and preflight fails nonretryably with unsupported before browser acquisition." "quality" api_007
jt_register "API-008" "agent-api" "Raw policy across surfaces" "Verify both large-surface and small-surface validate raw-CDP configuration while exposing it through their respective contracts." "Agent catalog config parser is available." "Resolve large-surface with raw enabled and verify cdp-call publication, then verify invalid raw values are rejected by both surfaces." "Large-surface publishes click plus cdp-call without small-surface builtins; both surfaces reject invalid raw policy values." "quality" api_008
jt_register "API-009" "agent-api" "Semantic read through browser-call" "Verify a small-surface semantic observation executes end-to-end through BrowserSession and browser-call." "Headless browser-perf fixture is loaded." "Execute read-page through browser-call with raw CDP disabled." "Batch completes and returns the fixture title through structured semantic result data." "browser" api_009
jt_register "API-010" "agent-api" "Semantic mutate and verify batch" "Verify ordered semantic mutation and verification execute in one small-surface browser-call batch." "Headless browser-perf fixture is loaded." "Set document title with evaluate-js then assert-title in the same semantic batch." "Both calls succeed and verification observes the mutation." "browser" api_010
jt_register "API-011" "agent-api" "Whole-batch stateful preflight" "Verify logical-target validation occurs before any browser-call side effect." "Headless browser-perf fixture has only main." "Batch a title mutation before a semantic call targeting nonexistent tab-999." "browser-call returns target_not_found and the earlier title mutation never executes." "browser" api_011
jt_register "API-012" "agent-api" "Stop failure policy" "Verify browser-call on_error=stop records the first semantic failure and prevents later side effects." "Headless browser-perf fixture is loaded." "Fail assert-title then schedule a title mutation under on_error=stop." "Batch status is stopped, only one call is attempted, and title remains unchanged." "browser" api_012
jt_register "API-013" "agent-api" "Continue failure policy" "Verify browser-call on_error=continue preserves structured failure while executing later independent calls." "Headless browser-perf fixture is loaded." "Fail assert-title then mutate title under on_error=continue." "Batch completes_with_errors, attempts both calls, and later mutation is observable." "browser" api_013
jt_register "API-014" "agent-api" "Stale reference typed failure" "Verify small-surface semantic calls preserve document-scoped ref staleness and typed recovery signals." "Headless browser-perf fixture is loaded and page runtime refs are available." "Snapshot interactive controls, remove the referenced button, then click its prior ref through browser-call." "The semantic call returns structured target_stale rather than acting on a replacement element." "browser" api_014
jt_register "API-015" "agent-api" "Logical-target routing and raw invalidation recovery" "Verify target-scoped raw CDP addresses an auto-attached logical tab without mutating the active page, and raw browser-level target destruction reconciles state deterministically." "Headless browser-perf fixture is loaded; raw CDP is enabled for the probe." "Create a second target, execute target-scoped Runtime.evaluate, close it through browser-scoped Target.closeTarget, then retry the destroyed logical label and read main." "Targeted call does not change the active tab; destroyed label is removed; reuse fails retryably with target_not_found; main remains operational." "browser" api_015
jt_register "API-016" "agent-api" "Raw target execution" "Verify enabled target-scoped raw CDP executes through browser-call and preserves structured result shape." "Headless browser-perf fixture is loaded; raw CDP is enabled for the probe." "Execute Runtime.evaluate 21*2 with target scope." "CDP call succeeds and returns value 42 in the ordered result." "browser" api_016
jt_register "API-017" "agent-api" "Raw browser execution" "Verify enabled browser-scoped raw CDP executes on the browser connection rather than a page session." "Headless browser-perf fixture is loaded; raw CDP is enabled for the probe." "Execute Target.getTargets with browser scope." "Structured CDP result contains targetInfos." "browser" api_017
jt_register "API-018" "agent-api" "Mixed semantic and raw ordering" "Verify one browser-call batch preserves ordering across semantic and raw CDP operations." "Headless browser-perf fixture is loaded; raw CDP is enabled for the probe." "Set a page variable semantically, increment it through raw target CDP, then read it semantically." "Batch kind is mixed, all three calls succeed in order, and final value is 15." "browser" api_018
jt_register "API-019" "agent-api" "Event subscription lifecycle" "Verify browser-events subscribe/poll/unsubscribe integrates with small-surface semantic execution and logical target attribution." "Headless browser-perf fixture is loaded; Runtime events can be enabled on main." "Subscribe to main Runtime.consoleAPICalled, emit an event through semantic browser-call, poll, then unsubscribe." "Poll contains the expected main event and unsubscribe completes successfully." "browser" api_019
jt_register "API-020" "agent-api" "Stale event subscription failure" "Verify removed browser-events subscription IDs fail deterministically with the dedicated typed error." "Headless browser-perf fixture is loaded." "Subscribe, unsubscribe, then poll the same subscription ID." "Poll fails nonretryably with subscription_not_found." "browser" api_020
jt_register "API-021" "agent-api" "Small-surface is the process default" "Verify an MCP process with no surface selector publishes the small-surface browser facade and not large-surface individual primitives." "Agent API probe can spawn a fresh subprocess with JELLY_MCP_SURFACE and JELLY_MCP_RAW_CDP removed." "Run active tools/list selection in the fresh process without either environment variable." "browser-schema, browser-call, and browser-events are published; click is not published." "quality" api_021
jt_register "API-022" "agent-api" "Explicit large-surface process with raw CDP" "Verify JELLY_MCP_SURFACE=large-surface still publishes individual primitives and can additionally expose raw CDP through cdp-call when JELLY_MCP_RAW_CDP=1." "Agent API probe can spawn a fresh subprocess." "Run active tools/list with JELLY_MCP_SURFACE=large-surface and JELLY_MCP_RAW_CDP=1." "The subprocess succeeds, publishes click plus cdp-call, and excludes browser-schema/browser-call/browser-events." "quality" api_022
