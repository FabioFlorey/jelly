#!/usr/bin/env bash

WEB_SELENIUM_URL="https://www.selenium.dev/selenium/web/web-form.html"
WEB_PORSCHE_URL="https://www.porsche.com/italy/"

web_open_ready() {
  local url="$1" opened=false
  jt_close_browser
  for _ in {1..4}; do
    if "$BIN_DIR/agent-open-browser" --headless "$url" >/dev/null 2>&1 \
      && "$BIN_DIR/agent-evaluate-js" 'document.readyState' >/dev/null 2>&1; then
      opened=true
      break
    fi
    jt_close_browser
    sleep 0.5
  done
  jt_assert_eq "$opened" "true" "browser must become ready for $url"
}

web_selenium_form() {
  trap jt_close_browser EXIT
  web_open_ready "$WEB_SELENIUM_URL"
  local snapshot text_ref select_ref check_ref disabled_ref readonly_ref state
  snapshot="$(jt_snapshot 100 0)"
  jt_assert_ge "$(jq 'length' <<<"$snapshot")" "17" "Selenium form must expose broad control coverage"
  text_ref="$(jt_ref_for_name 'Text input' <<<"$snapshot")"
  select_ref="$(jq -r '.[]|select(.role=="combobox" and (.name|startswith("Dropdown (select)")))|.ref' <<<"$snapshot")"
  check_ref="$(jt_ref_for_name 'Default checkbox' <<<"$snapshot")"
  disabled_ref="$(jt_ref_for_name 'Disabled input' <<<"$snapshot")"
  readonly_ref="$(jt_ref_for_name 'Readonly input' <<<"$snapshot")"
  for ref in "$text_ref" "$select_ref" "$check_ref" "$disabled_ref" "$readonly_ref"; do jt_assert_nonempty "$ref" "Selenium target ref missing"; done
  "$BIN_DIR/agent-fill" 'Jelly regression' "$text_ref" >/dev/null
  "$BIN_DIR/agent-select" "$select_ref" Two >/dev/null
  "$BIN_DIR/agent-check" "$check_ref" >/dev/null
  state="$(jt_runtime_eval "({text:document.querySelector('[name=my-text]').value,selectValue:document.querySelector('[name=my-select]').value,selectText:document.querySelector('[name=my-select]').selectedOptions[0]?.textContent.trim(),checked:document.querySelector('#my-check-2').checked})")"
  jt_assert_eq "$(jq -r '.text' <<<"$state")" "Jelly regression" "fill must change real form value"
  jt_assert_eq "$(jq -r '.selectValue' <<<"$state")" "2" "select must change real DOM value"
  jt_assert_eq "$(jq -r '.selectText' <<<"$state")" "Two" "selected option text must match"
  jt_assert_true "$(jq -r '.checked' <<<"$state")" "check must change real checkbox state"
  jt_expect_failure "disabled input must reject fill" env "$BIN_DIR/agent-fill" nope "$disabled_ref"
  jt_expect_failure "readonly input must reject fill" env "$BIN_DIR/agent-fill" nope "$readonly_ref"
}

web_selenium_artifacts() {
  trap 'if [[ -f $CONFIG_RUNTIME_ROOT/artifacts/recordings/active.json ]]; then "$BIN_DIR/agent-record-browser" stop >/dev/null 2>&1 || true; fi; jt_close_browser' EXIT
  web_open_ready "$WEB_SELENIUM_URL"
  local snapshot submit_ref link_ref shot tabs_before tabs_open tabs_closed recording record_path manifest_path new_target manifest_final_target manifest_final_title
  snapshot="$(jt_snapshot 100 0)"
  submit_ref="$(jt_ref_for_name 'Submit' <<<"$snapshot")"
  link_ref="$(jt_ref_for_name 'Return to index' <<<"$snapshot")"
  jt_assert_nonempty "$submit_ref" "Submit ref missing"
  jt_assert_nonempty "$link_ref" "Return-to-index ref missing"
  "$BIN_DIR/agent-highlight" "$submit_ref" 'Submit regression' >/dev/null
  jt_assert_true "$(jt_runtime_eval "!!document.querySelector('[data-jelly-highlight=true]')")" "runtime-ref highlight must be installed"
  shot="$("$BIN_DIR/agent-screenshot" "$submit_ref")"
  [[ -s "$shot" ]] || jt_fail "element screenshot artifact must exist and be non-empty: $shot"
  "$BIN_DIR/agent-clear-highlight" >/dev/null
  tabs_before="$("$BIN_DIR/agent-tabs" | sed '/^[[:space:]]*$/d' | wc -l | tr -d ' ')"
  "$BIN_DIR/agent-record-browser" start --mode continuous --interval-ms 200 >/dev/null
  "$BIN_DIR/agent-highlight" "$submit_ref" 'Submit regression' >/dev/null
  sleep 0.3
  "$BIN_DIR/agent-clear-highlight" >/dev/null
  "$BIN_DIR/agent-open-in-new-tab" "$link_ref" >/dev/null
  tabs_open="$("$BIN_DIR/agent-tabs" | sed '/^[[:space:]]*$/d' | wc -l | tr -d ' ')"
  jt_assert_eq "$tabs_open" "$((tabs_before+1))" "open-in-new-tab must add one tab"
  new_target="$(cat "$CONFIG_RUNTIME_ROOT/state/active_target_id")"
  jt_assert_eq "$(jt_runtime_eval 'document.title = "Jelly recording target"; document.title' | jq -r '.')" "Jelly recording target" "secondary tab title marker must be applied"
  sleep 0.6
  recording="$("$BIN_DIR/agent-record-browser" stop)"
  record_path="$(jq -r '.path' <<<"$recording")"
  manifest_path="$(jq -r '.manifest_path' <<<"$recording")"
  [[ -s "$record_path" && -s "$manifest_path" ]] || jt_fail "recording and manifest must exist"
  jt_assert_true "$(jq -r '.verification.integrity_verified' <<<"$recording")" "recording integrity must verify"
  jt_assert_ge "$(jq -r '.properties.frame_count' <<<"$recording")" "2" "recording must capture multiple frames"
  manifest_final_target="$(jq -r '.final_target_id // empty' "$manifest_path")"
  manifest_final_title="$(jq -r '.final_title // empty' "$manifest_path")"
  jt_assert_eq "$manifest_final_target" "$new_target" "continuous recording must follow Jelly's externally selected active target"
  jt_assert_eq "$manifest_final_title" "Jelly recording target" "continuous recording final context must match the followed target"
  "$BIN_DIR/agent-close-tab" >/dev/null
  tabs_closed="$("$BIN_DIR/agent-tabs" | sed '/^[[:space:]]*$/d' | wc -l | tr -d ' ')"
  jt_assert_eq "$tabs_closed" "$tabs_before" "close-tab must restore tab count"
  jt_assert_eq "$(jt_runtime_eval 'document.title' | jq -r '.')" "Web form" "closing tab must return to original page"
  jt_assert_eq "$(jt_runtime_eval 'document.visibilityState' | jq -r '.')" "visible" "page must remain usable after artifacts/tabs"
}

web_porsche_consent() {
  local profile="$CONFIG_RUNTIME_ROOT/profiles/headless" backup="/tmp/jelly-suite-porsche-profile-$$" moved=false
  rm -rf "$backup"
  jt_close_browser
  if [[ -d "$profile" ]]; then mv "$profile" "$backup"; moved=true; fi
  if [[ "$moved" == true ]]; then
    trap "jt_close_browser; rm -rf '$profile'; mv '$backup' '$profile'" EXIT
  else
    trap jt_close_browser EXIT
  fi
  web_open_ready "$WEB_PORSCHE_URL"
  local snapshot='' found=false ref info after
  for _ in $(seq 1 30); do
    snapshot="$(jt_snapshot 500 0 || true)"
    if jq -e '.[]|select(.name=="Solo cookie necessari")' <<<"$snapshot" >/dev/null 2>&1; then found=true; break; fi
    sleep 1
  done
  jt_assert_eq "$found" "true" "fresh-profile Porsche consent control must become discoverable"
  ref="$(jt_ref_for_name 'Solo cookie necessari' <<<"$snapshot")"
  info="$("$BIN_DIR/agent-element-info" 'text:Solo cookie necessari')"
  jt_assert_eq "$(jq -r '.tag' <<<"$info")" "button" "consent text must resolve real button"
  jt_assert_true "$(jq -r --arg ref "$ref" '.[]|select(.ref==$ref)|.shadow' <<<"$snapshot")" "consent button must be discovered through open Shadow DOM"
  "$BIN_DIR/agent-click" 'text:Solo cookie necessari' >/dev/null
  for _ in {1..20}; do
    after="$(jt_find 'Solo cookie necessari' 10 0)"
    [[ "$(jq 'length' <<<"$after")" == 0 ]] && return 0
    sleep 0.1
  done
  jt_fail "consent control did not disappear after click"
}

jt_register "WEB-001" "real-world" "Selenium form interactions" "Exercise real public form discovery, fill/select/check state changes, and disabled/readonly rejection." "Network access to selenium.dev; fresh headless browser." "Interact with Text input, Dropdown, Default checkbox, Disabled input and Readonly input." "Mutable controls change real DOM state; disabled/readonly fills fail." "network" web_selenium_form
jt_register "WEB-002" "real-world" "Selenium artifacts, tabs and handoff" "Exercise real-page highlight, element screenshot, tab lifecycle and continuous recording while preserving browser usability and following Jelly's shared active target." "Network access to selenium.dev; runtime refs for Submit and Return to index." "Highlight/screenshot Submit, start continuous recording, record highlight activity, switch to a new active tab, then stop and close it." "Artifacts are valid, recording captures >=2 frames and ends on the externally selected active target, tab count restores, and the original page remains visible." "network" web_selenium_artifacts
jt_register "WEB-003" "real-world" "Porsche consent Shadow DOM" "Exercise live nested/open Shadow DOM discovery and click against Porsche/Usercentrics using a fresh browser profile." "Network access to porsche.com; profile directory can be temporarily moved." "Discover and click 'Solo cookie necessari'." "Control resolves as shadow button and disappears after click." "network" web_porsche_consent
