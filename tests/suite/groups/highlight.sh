#!/usr/bin/env bash

highlight_prepare() {
  jt_with_browser_cleanup
  jt_open_fixture highlight-modes.html
  HIGHLIGHT_SNAPSHOT="$(jt_snapshot 50 0)"
  HIGHLIGHT_LINK_REF="$(jq -r '.[]|select(.name|contains("Primary result title"))|.ref' <<<"$HIGHLIGHT_SNAPSHOT" | head -1)"
  HIGHLIGHT_CONTROL_REF="$(jq -r '.[]|select(.name=="Action control")|.ref' <<<"$HIGHLIGHT_SNAPSHOT" | head -1)"
  jt_assert_nonempty "$HIGHLIGHT_LINK_REF" "complex link ref missing"
  jt_assert_nonempty "$HIGHLIGHT_CONTROL_REF" "control ref missing"
}

highlight_auto_principal() {
  highlight_prepare
  local auto auto_width box_width
  auto="$(JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-highlight" "$HIGHLIGHT_LINK_REF" '' auto)"
  jt_assert_eq "$(jq -r '.resolved_mode' <<<"$auto")" "content" "auto must choose content mode for complex text link"
  jt_assert_eq "$(jq -r '.fragment_count' <<<"$auto")" "1" "auto must isolate principal title fragment"
  auto_width="$(jq -r '.fragments[0].width' <<<"$auto")"
  box_width="$(jq -r '.geometry.width' <<<"$auto")"
  jt_assert_lt "$auto_width" "$box_width" "principal title halo must be tighter than anchor box"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-clear-highlight" >/dev/null
}

highlight_modes() {
  highlight_prepare
  local content box shape
  content="$(JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-highlight" "$HIGHLIGHT_LINK_REF" '' content)"
  jt_assert_ge "$(jq -r '.fragment_count' <<<"$content")" "3" "content mode must retain all rendered text regions"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-clear-highlight" >/dev/null
  box="$(JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-highlight" "$HIGHLIGHT_LINK_REF" '' box)"
  jt_assert_eq "$(jq -r '.resolved_mode' <<<"$box")" "box" "explicit box mode must remain box"
  jt_assert_eq "$(jq -r '.fragment_count' <<<"$box")" "1" "box mode must use one rectangle"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-clear-highlight" >/dev/null
  shape="$(JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-highlight" "$HIGHLIGHT_CONTROL_REF" '' auto)"
  jt_assert_eq "$(jq -r '.resolved_mode' <<<"$shape")" "shape" "auto must choose shape for button"
  jt_assert_eq "$(jq -r '.fragment_count' <<<"$shape")" "1" "shape mode must use one halo"
}

highlight_follow_cleanup() {
  highlight_prepare
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-highlight" "$HIGHLIGHT_LINK_REF" '' auto >/dev/null
  jt_runtime_eval "window.__jellyFragmentProbe=window.__jellyHighlight.root.querySelector('[data-jelly-highlight-fragment]');window.__jellyBeforeRect=window.__jellyFragmentProbe.getBoundingClientRect().toJSON();true" >/dev/null
  jt_runtime_eval "document.querySelector('#complex-link').style.transform='translate(80px, 40px)'; true" >/dev/null
  sleep 0.12
  jt_assert_true "$(jt_runtime_eval "window.__jellyFragmentProbe===window.__jellyHighlight.root.querySelector('[data-jelly-highlight-fragment]')")" "animation updates must reuse fragment DOM node"
  local moved
  moved="$(jt_runtime_eval "(()=>{const a=window.__jellyBeforeRect,b=window.__jellyFragmentProbe.getBoundingClientRect();return b.x>a.x+50&&b.y>a.y+20})()")"
  jt_assert_true "$moved" "highlight fragment must follow target layout movement"
  jt_runtime_eval "document.querySelector('#complex-link').remove(); true" >/dev/null
  sleep 0.12
  jt_assert_eq "$(jt_runtime_eval "document.querySelectorAll('[data-jelly-highlight=true]').length")" "0" "removing target must clean highlight overlay on animation frame"
  jt_assert_false "$(jt_runtime_eval '!!window.__jellyHighlight')" "removing target must clear global highlight state"
}

highlight_visibility_validation() {
  highlight_prepare
  local style_before style_after clear
  style_before="$(jt_runtime_eval "document.querySelector('#complex-link').getAttribute('style')")"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-highlight" "$HIGHLIGHT_LINK_REF" 'Label' auto >/dev/null
  style_after="$(jt_runtime_eval "document.querySelector('#complex-link').getAttribute('style')")"
  jt_assert_eq "$style_after" "$style_before" "highlight must not mutate target inline style"
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-clear-highlight" >/dev/null
  jt_runtime_eval "document.querySelector('#control').style.opacity='0'; true" >/dev/null
  jt_expect_failure "opacity:0 target must be rejected as not visibly rendered" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-highlight" "$HIGHLIGHT_CONTROL_REF" '' auto
  jt_runtime_eval "document.querySelector('#control').style.opacity=''; true" >/dev/null
  jt_expect_failure "invalid highlight mode must fail" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-highlight" "$HIGHLIGHT_LINK_REF" '' nope
  JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-highlight" "$HIGHLIGHT_LINK_REF" '' auto >/dev/null
  clear="$($JELLY_BIN_DIR/agent-clear-highlight)"
  jt_assert_true "$(jq -r '.cleared' <<<"$clear")" "clear-highlight must report active overlay cleared"
  jt_assert_eq "$(jt_runtime_eval "document.querySelectorAll('[data-jelly-highlight=true]').length")" "0" "clear-highlight must leave no overlay root"
  clear="$($JELLY_BIN_DIR/agent-clear-highlight)"
  jt_assert_false "$(jq -r '.cleared' <<<"$clear")" "clearing with no active overlay must be idempotent and report false"
}

jt_register "HLT-001" "highlight" "Auto principal-text halo" "Verify auto mode recognizes a text-centric complex link and isolates its principal visible title rather than metadata/detail." "highlight-modes.html complex-link fixture." "highlight <complex-link-ref> auto" "resolved_mode=content, exactly one title fragment, fragment narrower than anchor box." "browser" highlight_auto_principal
jt_register "HLT-002" "highlight" "Explicit highlight modes" "Verify content, box and auto-shape modes produce the intended fragment geometry." "Complex link and button refs available." "highlight content/box on link and auto on button." "Content returns all text regions; box returns one rectangle; button auto resolves to one shape halo." "browser" highlight_modes
jt_register "HLT-003" "highlight" "Animation follow and lifecycle cleanup" "Verify RAF updates reuse overlay nodes, follow target movement, and remove overlay/global state when the target disconnects." "Active auto highlight on connected complex link." "Move target with CSS transform, then remove target." "Same fragment node moves with target; disconnected target causes overlay and __jellyHighlight cleanup." "browser" highlight_follow_cleanup
jt_register "HLT-004" "highlight" "Visibility, non-mutation and clearing" "Verify highlight does not mutate targets, rejects opacity-zero targets/invalid modes, and clear is complete/idempotent." "Highlight fixture with visible refs." "Highlight/clear, set button opacity:0, call invalid mode, clear twice." "Target style remains unchanged; invisible/invalid calls fail; first clear reports true and removes overlay; second reports false." "browser" highlight_visibility_validation
