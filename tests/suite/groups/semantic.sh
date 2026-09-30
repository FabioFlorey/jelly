#!/usr/bin/env bash

semantic_prepare() {
  jt_with_browser_cleanup
  jt_open_fixture semantic-targets.html
  jt_snapshot 300 0 >/dev/null
}

semantic_expect_tag() {
  local query="$1" tag="$2"
  local result
  result="$(jt_find "$query" 20 0)"
  jt_assert_eq "$(jq -r '.[0].tag // ""' <<<"$result")" "$tag" "semantic query '$query' must resolve expected tag"
}

semantic_names() {
  semantic_prepare
  semantic_expect_tag "Visible action" button
  semantic_expect_tag "Email label" input
  semantic_expect_tag "ARIA action" button
  semantic_expect_tag "Labelled action" button
  semantic_expect_tag "Search placeholder" input
  semantic_expect_tag "Value target" input
  semantic_expect_tag "Sibling label action" input
  semantic_expect_tag "Image action" img
  semantic_expect_tag "Title action" div
}

semantic_ranking() {
  semantic_prepare
  local duplicate viewport priority save ids
  duplicate="$(jt_find 'Duplicate action' 5 0)"
  jt_assert_false "$(jq -r '.[0].disabled' <<<"$duplicate")" "enabled duplicate must rank before disabled duplicate"
  jt_assert_eq "$(JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-get-element" "$(jq -r '.[0].ref' <<<"$duplicate")" html | grep -c 'id="duplicate-enabled"')" "1" "enabled duplicate must be selected"
  viewport="$(jt_find 'Viewport action' 5 0)"
  jt_assert_true "$(jq -r '.[0].in_viewport' <<<"$viewport")" "onscreen duplicate must rank before offscreen equal-actionability duplicate"
  priority="$(jt_find 'Priority action' 5 0)"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-get-element" "$(jq -r '.[0].ref' <<<"$priority")" html | grep -q 'priority-enabled-offscreen' || jt_fail "enabled offscreen duplicate must outrank disabled onscreen duplicate"
  save="$(jt_find 'Save' 10 0)"
  ids="$(for ref in $(jq -r '.[0:3][].ref' <<<"$save"); do JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-get-element" "$ref" html | sed -n 's/.*id="\([^"]*\)".*/\1/p'; done | paste -sd, -)"
  jt_assert_eq "$ids" "rank-exact,rank-prefix,rank-contains" "match quality must order exact before prefix before contains"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-get-element" 'text:Duplicate action' html | grep -q 'duplicate-enabled' || jt_fail "direct text target must use enabled-first interactive ranking"
}

semantic_generic_fallback() {
  semantic_prepare
  local before indexed after_indexed after_generic missing_after generic whitespace choice legacy
  before="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.metrics')"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" 'text:Visible action' >/dev/null
  indexed="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.metrics')"
  jt_assert_eq "$(jq -r '.genericTextFallbacks' <<<"$indexed")" "$(jq -r '.genericTextFallbacks' <<<"$before")" "indexed interactive text must not trigger generic fallback"
  generic="$(JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-get-element" 'text:Generic status text' html)"
  grep -q 'id="generic-visible"' <<<"$generic" || jt_fail "generic fallback must resolve visible generic text"
  whitespace="$(JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-get-element" 'text:Whitespace generic target' html)"
  grep -q 'id="generic-whitespace"' <<<"$whitespace" || jt_fail "generic fallback must normalize rendered whitespace"
  after_generic="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.metrics')"
  jt_assert_eq "$(( $(jq -r '.genericTextFallbacks' <<<"$after_generic") - $(jq -r '.genericTextFallbacks' <<<"$indexed") ))" "2" "two generic resolutions must record two fallbacks"
  jt_assert_eq "$(( $(jq -r '.genericTextPrefilterHits' <<<"$after_generic") - $(jq -r '.genericTextPrefilterHits' <<<"$indexed") ))" "2" "visible generic cases must hit cheap prefilter"
  choice="$(JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-get-element" 'text:Container choice' html)"
  grep -q 'id="interactive-container-choice"' <<<"$choice" || jt_fail "interactive exact match must outrank generic container"
  jt_expect_failure "missing generic target must fail" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" 'text:Definitely missing target'
  missing_after="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.metrics')"
  jt_assert_eq "$(( $(jq -r '.genericTextSlowFallbacks' <<<"$missing_after") - $(jq -r '.genericTextSlowFallbacks' <<<"$after_generic") ))" "1" "missing target must reach slow fallback"
  jt_assert_eq "$(( $(jq -r '.genericTextFallbackMisses' <<<"$missing_after") - $(jq -r '.genericTextFallbackMisses' <<<"$after_generic") ))" "1" "missing target must record fallback miss"
  legacy="$(JELLY_PAGE_RUNTIME=0 "$JELLY_BIN_DIR/agent-get-element" 'text:Generic status text' html)"
  grep -q 'id="generic-visible"' <<<"$legacy" || jt_fail "legacy mode must preserve generic text targeting"
}

semantic_visibility() {
  semantic_prepare
  local mode label
  for mode in 1 0; do
    label="$([[ "$mode" == 1 ]] && echo runtime || echo legacy)"
    jt_assert_eq "$(JELLY_PAGE_RUNTIME="$mode" "$JELLY_BIN_DIR/agent-find-interactive" 'Hidden generic text' 10 | jq 'length')" "0" "$label display:none generic text must not be interactive"
    jt_expect_failure "$label display:none generic text target must not resolve" env JELLY_PAGE_RUNTIME="$mode" "$JELLY_BIN_DIR/agent-element-info" 'text:Hidden generic text'
    jt_assert_eq "$(JELLY_PAGE_RUNTIME="$mode" "$JELLY_BIN_DIR/agent-find-interactive" 'Opacity hidden action' 10 | jq 'length')" "0" "$label opacity:0 interactive control must not be returned by semantic search"
    jt_expect_failure "$label opacity:0 text target must not resolve as visible" env JELLY_PAGE_RUNTIME="$mode" "$JELLY_BIN_DIR/agent-assert-visible" 'text:Opacity hidden action'
  done
}

semantic_scroll_duplicate_text() {
  semantic_prepare
  local before after target_top vh
  before="$(jt_runtime_eval "({summary:document.querySelector('#references-summary').getBoundingClientRect().top,target:document.querySelector('#references-section').getBoundingClientRect().top,scrollY})")"
  jt_assert_gt "$(jq -r '.target' <<<"$before")" "1000" "references section must begin offscreen"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-scroll" 'text:References' >/dev/null
  after="$(jt_runtime_eval "({summary:document.querySelector('#references-summary').getBoundingClientRect().top,target:document.querySelector('#references-section').getBoundingClientRect().top,scrollY,innerHeight})")"
  target_top="$(jq -r '.target' <<<"$after")"
  vh="$(jq -r '.innerHeight' <<<"$after")"
  awk -v y="$target_top" -v h="$vh" 'BEGIN { exit !(y >= 0 && y < h) }' || jt_fail "scroll text:References must prefer the actual section heading over the earlier duplicate summary text"
  jt_assert_gt "$(jq -r '.scrollY' <<<"$after")" "0" "scroll must actually move the document to the section"
}

jt_register "SEM-001" "semantic" "Accessible-name sources" "Verify semantic indexing derives names from rendered text, labels, ARIA, placeholder/value, adjacent labels, alt text and title." "semantic-targets.html fixture; runtime index installed." "Search each fixture name with find-interactive." "Each query returns the intended element type and accessible-name source." "browser" semantic_names
jt_register "SEM-002" "semantic" "Interactive ranking precedence" "Verify exact/prefix/contains match quality plus enabled and viewport ranking rules are deterministic." "Semantic fixture contains controlled duplicate and match-quality candidates." "Search Duplicate action, Viewport action, Priority action and Save." "Enabled outranks disabled; viewport breaks equal-actionability ties; exact > prefix > contains; direct text uses same interactive choice." "browser" semantic_ranking
jt_register "SEM-003" "semantic" "Generic text fallback" "Verify non-interactive text fallback, whitespace normalization, metrics, miss accounting and legacy compatibility." "Runtime installed on semantic fixture." "Resolve generic visible text, whitespace-normalized text, container collision and a missing target." "Generic text resolves through prefilter, metrics increment correctly, interactive exact match wins, miss reaches slow pass, and legacy mode remains compatible." "browser" semantic_generic_fallback
jt_register "SEM-004" "semantic" "Visibility consistency" "Verify display:none and opacity:0 content are consistently excluded from semantic discovery/visibility." "Semantic fixture contains hidden and opacity-zero targets." "find-interactive and assert-visible on hidden targets." "Neither hidden target is returned as a visible semantic action; visibility assertion fails." "browser" semantic_visibility
jt_register "SEM-005" "semantic" "Duplicate section-text scrolling" "Verify scroll-by-text targets the semantic section heading rather than an earlier duplicate summary/TOC text node." "Fixture has visible duplicate 'References' summary and offscreen h2 References section." "scroll text:References" "Document scrolls and the h2 References section lands inside the viewport." "browser" semantic_scroll_duplicate_text
