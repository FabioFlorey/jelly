#!/usr/bin/env bash

shadow_prepare() {
  jt_with_browser_cleanup
  jt_open_fixture shadow-targets.html
}

shadow_refs() {
  jt_snapshot 100 0
}

shadow_discovery() {
  shadow_prepare
  local snapshot
  snapshot="$(shadow_refs)"
  for name in "Shadow click" "Shadow input" "Shadow check" "Shadow select" "Shadow scroll target" "Nested shadow click" "Slotted shadow action"; do
    jt_assert_nonempty "$(jt_ref_for_name "$name" <<<"$snapshot")" "missing open/nested/slotted shadow ref for $name"
  done
  jt_assert_eq "$(jt_find 'Closed shadow action' 10 0 | jq 'length')" "0" "pre-existing closed shadow root must remain opaque"
  local slotted
  slotted="$("$BIN_DIR/agent-get-element" 'text:Slotted shadow action' html)"
  grep -q 'id="slotted-click"' <<<"$slotted" || jt_fail "slotted text must resolve inner shadow button"
}

shadow_interactions() {
  shadow_prepare
  local snapshot click_ref input_ref check_ref select_ref slotted_ref
  snapshot="$(shadow_refs)"
  click_ref="$(jt_ref_for_name 'Shadow click' <<<"$snapshot")"
  input_ref="$(jt_ref_for_name 'Shadow input' <<<"$snapshot")"
  check_ref="$(jt_ref_for_name 'Shadow check' <<<"$snapshot")"
  select_ref="$(jt_ref_for_name 'Shadow select' <<<"$snapshot")"
  slotted_ref="$(jt_ref_for_name 'Slotted shadow action' <<<"$snapshot")"
  "$BIN_DIR/agent-click" "$slotted_ref" >/dev/null
  jt_assert_eq "$(jt_runtime_eval 'window.shadowClicks')" "10" "slotted shadow click must hit intended button"
  "$BIN_DIR/agent-click" "$click_ref" >/dev/null
  jt_assert_eq "$(jt_runtime_eval 'window.shadowClicks')" "11" "CDP click must hit open-shadow button"
  "$BIN_DIR/agent-fill" 'filled through Jelly' "$input_ref" >/dev/null
  jt_assert_eq "$(jt_runtime_eval "document.querySelector('#open-host').shadowRoot.querySelector('#shadow-input').value")" '"filled through Jelly"' "fill must mutate shadow input"
  "$BIN_DIR/agent-check" "$check_ref" >/dev/null
  jt_assert_true "$(jt_runtime_eval "document.querySelector('#open-host').shadowRoot.querySelector('#shadow-check').checked")" "check must mutate shadow checkbox"
  "$BIN_DIR/agent-select" "$select_ref" Two >/dev/null
  jt_assert_eq "$(jt_runtime_eval "document.querySelector('#open-host').shadowRoot.querySelector('#shadow-select').value")" '"two"' "select must mutate shadow select"
}

shadow_visual_geometry() {
  shadow_prepare
  local snapshot scroll_ref nested_ref after_scroll visible highlight nested_info artifact shot_width shot_height coord_match
  snapshot="$(shadow_refs)"
  scroll_ref="$(jt_ref_for_name 'Shadow scroll target' <<<"$snapshot")"
  nested_ref="$(jt_ref_for_name 'Nested shadow click' <<<"$snapshot")"
  "$BIN_DIR/agent-scroll" "$scroll_ref" >/dev/null
  after_scroll="$("$BIN_DIR/agent-element-info" "$scroll_ref")"
  visible="$(jq -n --argjson y "$(jq '.rect.y' <<<"$after_scroll")" --argjson h "$(jq '.rect.height' <<<"$after_scroll")" --argjson vh "$(jt_runtime_eval 'innerHeight')" '($y+$h)>0 and $y<$vh')"
  jt_assert_true "$visible" "shadow scroll target must land in viewport"
  highlight="$("$BIN_DIR/agent-highlight" "$nested_ref" 'nested target')"
  jt_assert_true "$(jq -r '.highlighted' <<<"$highlight")" "nested shadow ref must highlight"
  jt_assert_eq "$(jt_runtime_eval "document.querySelectorAll('[data-jelly-highlight=true]').length")" "1" "highlight overlay must be installed in light document"
  nested_info="$("$BIN_DIR/agent-element-info" "$nested_ref")"
  coord_match="$(jq -n --argjson hx "$(jq '.geometry.x' <<<"$highlight")" --argjson hy "$(jq '.geometry.y' <<<"$highlight")" --argjson ix "$(jq '.rect.x' <<<"$nested_info")" --argjson iy "$(jq '.rect.y' <<<"$nested_info")" '((($hx-$ix)|fabs)<0.01) and ((($hy-$iy)|fabs)<0.01)')"
  jt_assert_true "$coord_match" "highlight and element-info must agree on nested coordinates"
  "$BIN_DIR/agent-clear-highlight" >/dev/null
  artifact="$("$BIN_DIR/agent-screenshot" "$nested_ref" --output /tmp/jelly-suite-shadow.png --json)"
  jt_assert_eq "$(jq -r '.properties.mime' <<<"$artifact")" "image/png" "shadow element screenshot must be PNG"
  shot_width="$(jq -r '.properties.width' <<<"$artifact")"
  shot_height="$(jq -r '.properties.height' <<<"$artifact")"
  jt_assert_gt "$shot_width" "0" "screenshot width must be positive"
  jt_assert_gt "$shot_height" "0" "screenshot height must be positive"
  coord_match="$(jq -n --argjson sw "$shot_width" --argjson sh "$shot_height" --argjson rw "$(jq '.rect.width' <<<"$nested_info")" --argjson rh "$(jq '.rect.height' <<<"$nested_info")" '((($sw-$rw)|fabs)<=2) and ((($sh-$rh)|fabs)<=2)')"
  jt_assert_true "$coord_match" "screenshot clip must match nested element geometry"
  rm -f /tmp/jelly-suite-shadow.png
}

shadow_dynamic_stale() {
  shadow_prepare
  local snapshot nested_ref
  snapshot="$(shadow_refs)"
  nested_ref="$(jt_ref_for_name 'Nested shadow click' <<<"$snapshot")"
  jt_runtime_eval "(()=>{const h=document.querySelector('#late-open-host');const r=h.attachShadow({mode:'open'});const b=document.createElement('button');b.textContent='Late open action';r.append(b);return true})()" >/dev/null
  jt_assert_eq "$(jt_find 'Late open action' 10 0 | jq 'length')" "1" "late open shadow root must be indexed"
  jt_runtime_eval "(()=>{const h=document.querySelector('#late-closed-host');const r=h.attachShadow({mode:'closed'});const b=document.createElement('button');b.textContent='Late closed action';r.append(b);return true})()" >/dev/null
  jt_assert_eq "$(jt_find 'Late closed action' 10 0 | jq 'length')" "0" "late closed shadow root must stay opaque"
  jt_runtime_eval "document.querySelector('#open-host').shadowRoot.querySelector('#nested-host').remove(); true" >/dev/null
  jt_expect_failure "removed nested shadow ref must become stale" env "$BIN_DIR/agent-element-info" "$nested_ref"
}

jt_register "SHD-001" "shadow" "Shadow discovery and naming" "Verify open, nested and slotted shadow controls are indexed/named while closed roots remain opaque." "shadow-targets.html fixture." "Snapshot and semantic search across shadow roots." "All open/nested/slotted controls receive refs; closed control is absent; slotted text resolves inner button." "browser" shadow_discovery
jt_register "SHD-002" "shadow" "Shadow interactions" "Verify click, fill, check and select work on refs inside open Shadow DOM." "Shadow fixture with runtime refs." "Activate shadow button, fill input, check checkbox and select option." "Underlying shadow DOM state changes exactly as requested." "browser" shadow_interactions
jt_register "SHD-003" "shadow" "Shadow geometry and artifacts" "Verify scroll, highlight and element screenshot use correct coordinates for nested shadow targets." "Shadow fixture with offscreen and nested controls." "Scroll offscreen shadow target; highlight/screenshot nested target." "Scrolled target enters viewport; highlight coordinates equal element-info; PNG clip dimensions match target." "browser" shadow_visual_geometry
jt_register "SHD-004" "shadow" "Dynamic roots and stale descendants" "Verify late open roots become searchable, late closed roots remain opaque, and removed shadow descendants invalidate refs." "Runtime installed before late shadow roots are attached." "Attach open/closed roots, then remove nested host." "Open control appears, closed control does not, removed descendant ref fails." "browser" shadow_dynamic_stale
