#!/usr/bin/env bash

runtime_prepare_small() {
  jt_with_browser_cleanup
  jt_open_fixture browser-perf.html
  jt_runtime_eval "window.__jellyBench.setCase('small')" >/dev/null
}

runtime_ref_stability() {
  runtime_prepare_small
  local first second ref1 ref2 dom_refs
  first="$(jt_snapshot 200 0)"
  ref1="$(jt_ref_for_name 'small action 3' <<<"$first")"
  jt_assert_nonempty "$ref1" "initial runtime ref missing"
  second="$(jt_snapshot 200 0)"
  ref2="$(jt_ref_for_name 'small action 3' <<<"$second")"
  jt_assert_eq "$ref2" "$ref1" "same DOM node must keep its ref across snapshots"
  dom_refs="$(jt_runtime_eval "document.querySelectorAll('[data-jelly-ref]').length")"
  jt_assert_eq "$dom_refs" "0" "runtime refs must not mutate page attributes"
  jt_runtime_eval "(()=>{const d=document.createElement('div');d.textContent='unrelated';document.body.append(d);return true})()" >/dev/null
  jt_assert_eq "$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 200 0)")" "$ref1" "unrelated mutation must preserve element identity"
}

runtime_live_state_visibility() {
  runtime_prepare_small
  local first ref state
  first="$(jt_snapshot 200 0)"
  ref="$(jt_ref_for_name 'small action 3' <<<"$first")"
  jt_runtime_eval "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent==='small action 3');b.disabled=true;const c=document.createElement('div');c.textContent='custom checkbox';c.setAttribute('role','checkbox');c.setAttribute('tabindex','0');c.setAttribute('aria-label','ARIA live check');c.setAttribute('aria-checked','false');document.body.append(c);const a=document.createElement('div');a.textContent='custom button';a.setAttribute('role','button');a.setAttribute('tabindex','0');a.setAttribute('aria-label','ARIA disabled action');a.setAttribute('aria-disabled','false');document.body.append(a);const n=document.createElement('input');n.type='checkbox';n.setAttribute('aria-label','Native live check');document.body.append(n);return true})()" >/dev/null
  jt_snapshot 200 0 >/dev/null
  jt_runtime_eval "(()=>{document.querySelector('[aria-label=\"ARIA live check\"]').setAttribute('aria-checked','true');document.querySelector('[aria-label=\"ARIA disabled action\"]').setAttribute('aria-disabled','true');document.querySelector('[aria-label=\"Native live check\"]').checked=true;return true})()" >/dev/null
  state="$(jt_snapshot 200 0)"
  jt_assert_eq "$(jq -r '.[] | select(.name=="small action 3") | .disabled' <<<"$state")" "true" "native disabled state must be live"
  jt_assert_true "$(jq -r '.[]|select(.name=="ARIA live check")|.checked' <<<"$state")" "aria-checked state must be live"
  jt_assert_true "$(jq -r '.[]|select(.name=="ARIA disabled action")|.disabled' <<<"$state")" "aria-disabled state must be live"
  jt_assert_true "$(jq -r '.[]|select(.name=="Native live check")|.checked' <<<"$state")" "native checked state must be live"
  jt_runtime_eval "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent==='small action 3');b.style.display='none';return true})()" >/dev/null
  jt_assert_eq "$(jt_snapshot 200 0 | jq '[.[]|select(.name=="small action 3")]|length')" "0" "display:none control must not be returned"
  jt_runtime_eval "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent==='small action 3');b.style.display='';b.style.opacity='0';return true})()" >/dev/null
  jt_assert_eq "$(jt_snapshot 200 0 | jq '[.[]|select(.name=="small action 3")]|length')" "0" "opacity:0 control must not be returned as visible"
  jt_runtime_eval "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent==='small action 3');b.style.opacity='';b.disabled=false;b.style.marginTop='2500px';b.style.width='240px';return true})()" >/dev/null
  state="$(jt_snapshot 200 0)"
  jt_assert_eq "$(jq -r '.[]|select(.name=="small action 3")|.in_viewport' <<<"$state")" "false" "viewport state must be recomputed live"
  jt_assert_ge "$(jq -r '.[]|select(.name=="small action 3")|.width' <<<"$state")" "200" "geometry must be recomputed live"
  jt_runtime_eval "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent==='small action 3');b.style.marginTop='';b.style.width='';return true})()" >/dev/null
  jt_assert_eq "$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 200 0)")" "$ref" "visibility/layout changes must not churn ref identity"
}

runtime_replacement_navigation() {
  runtime_prepare_small
  local first old replacement after_nav fixture
  first="$(jt_snapshot 200 0)"
  old="$(jt_ref_for_name 'small action 3' <<<"$first")"
  jt_runtime_eval "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent==='small action 3');const c=b.cloneNode(true);b.replaceWith(c);return true})()" >/dev/null
  replacement="$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 200 0)")"
  jt_assert_ne "$replacement" "$old" "replacement node must receive new ref"
  jt_expect_failure "stale ref after replacement" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" "$old"
  fixture="file://${JELLY_REPO_ROOT}/tests/fixtures/browser-perf.html"
  "$JELLY_BIN_DIR/agent-navigate" "$fixture" >/dev/null
  jt_runtime_eval "window.__jellyBench.setCase('small')" >/dev/null
  after_nav="$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 200 0)")"
  jt_assert_ne "$after_nav" "$replacement" "navigation must create new ref namespace"
  jt_expect_failure "pre-navigation ref must be stale" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" "$replacement"
}

runtime_shadow_invalidation() {
  runtime_prepare_small
  jt_runtime_eval "(()=>{const h=document.createElement('div');h.id='late-shadow-host';document.body.append(h);return true})()" >/dev/null
  jt_snapshot 200 0 >/dev/null
  jt_runtime_eval "(()=>{const h=document.querySelector('#late-shadow-host');const r=h.attachShadow({mode:'open'});const b=document.createElement('button');b.textContent='late shadow target';r.append(b);return true})()" >/dev/null
  jt_assert_eq "$(jt_find 'late shadow target' 20 0 | jq 'length')" "1" "late open shadow root must invalidate and enter index"
  jt_runtime_eval "window.__jellyBench.setCase('shadow')" >/dev/null
  jt_assert_eq "$(jt_find 'nested shadow target' 20 0 | jq 'length')" "1" "nested open shadow roots must be searchable"
}

runtime_mutation_batching() {
  jt_with_browser_cleanup
  jt_open_fixture browser-perf.html
  jt_runtime_eval "window.__jellyBench.setCase('mutation')" >/dev/null
  jt_snapshot 200 0 >/dev/null
  local before state after
  before="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.epoch')"
  jt_runtime_eval "(()=>{const app=document.querySelector('#app');for(let i=0;i<120;i++){const b=document.createElement('button');b.textContent='batch action '+i;app.append(b)}for(const b of [...app.querySelectorAll('button')].slice(0,80)){b.setAttribute('aria-label','batched '+b.textContent)}return true})()" >/dev/null
  state="$(jt_runtime_eval '({epoch:globalThis.__jellyRuntimeV1.epoch,dirty:globalThis.__jellyRuntimeV1.dirty})')"
  after="$(jq -r '.epoch' <<<"$state")"
  jt_assert_eq "$((after-before))" "1" "batched synchronous mutations must coalesce to one invalidation epoch"
  jt_assert_true "$(jq -r '.dirty' <<<"$state")" "batch must mark runtime dirty"
  jt_snapshot 200 0 >/dev/null
  jt_assert_false "$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.dirty')" "next query must rebuild exactly once and clear dirty"
  jt_assert_eq "$(jt_find 'batch action 119' 20 0 | jq 'length')" "1" "new control must become searchable after rebuild"
}

runtime_semantic_name_updates() {
  runtime_prepare_small
  jt_snapshot 200 0 >/dev/null
  jt_runtime_eval "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent==='small action 3');b.setAttribute('aria-label','renamed action');return true})()" >/dev/null
  jt_assert_eq "$(jt_find 'renamed action' 20 0 | jq 'length')" "1" "ARIA label changes must invalidate semantic names"
  jt_runtime_eval "(()=>{const i=document.createElement('input');i.id='value-name-input';i.value='initial value name';document.body.append(i);return true})()" >/dev/null
  jt_snapshot 200 0 >/dev/null
  jt_runtime_eval "(()=>{const i=document.querySelector('#value-name-input');i.value='updated value name';i.dispatchEvent(new Event('input',{bubbles:true}));return true})()" >/dev/null
  jt_assert_eq "$(jt_find 'updated value name' 20 0 | jq 'length')" "1" "input event must invalidate value-derived name"
  jt_runtime_eval "(()=>{const i=document.createElement('input');i.id='silent-value-input';i.value='old silent value';document.body.append(i);return true})()" >/dev/null
  jt_snapshot 200 0 >/dev/null
  jt_runtime_eval "document.querySelector('#silent-value-input').value='new silent value'" >/dev/null
  jt_assert_eq "$(jt_snapshot 200 0 | jq -r '.[]|select(.tag=="input" and .name=="new silent value")|.name')" "new silent value" "snapshot must refresh silently changed value-derived names"
  jt_assert_eq "$(jt_find 'new silent value' 20 0 | jq 'length')" "1" "search must see silently changed value"
  jt_assert_eq "$(jt_find 'old silent value' 20 0 | jq 'length')" "0" "stale cached value name must be rejected"
}

runtime_rollback_reinstall() {
  runtime_prepare_small
  local runtime_ref legacy legacy_ref runtime_present native new_runtime
  runtime_ref="$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 200 0)")"
  legacy="$(JELLY_PAGE_RUNTIME=0 "$JELLY_BIN_DIR/agent-snapshot-interactive" 10)"
  legacy_ref="$(jq -r '.[0].ref' <<<"$legacy")"
  [[ "$legacy_ref" =~ ^@e[0-9]+$ ]] || jt_fail "rollback must restore legacy numeric refs"
  runtime_present="$(jt_runtime_eval '!!globalThis.__jellyRuntimeV1')"
  jt_assert_false "$runtime_present" "rollback must dispose page runtime"
  native="$(jt_runtime_eval "Function.prototype.toString.call(Element.prototype.attachShadow).includes('[native code]')")"
  jt_assert_true "$native" "rollback must restore native attachShadow"
  jt_assert_gt "$(jt_runtime_eval "document.querySelectorAll('[data-jelly-ref]').length")" "0" "legacy mode must restore DOM-backed refs"
  jt_snapshot 200 0 >/dev/null
  jt_assert_true "$(jt_runtime_eval '!!globalThis.__jellyRuntimeV1')" "runtime must reinstall after rollback"
  jt_assert_true "$(jt_runtime_eval 'Element.prototype.attachShadow===globalThis.__jellyRuntimeV1.attachShadowHook')" "reinstalled runtime must own exactly one active attachShadow hook"
  new_runtime="$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 200 0)")"
  jt_assert_ne "$new_runtime" "$runtime_ref" "reinstall must create fresh runtime namespace"
  JELLY_PAGE_RUNTIME=0 "$JELLY_BIN_DIR/agent-snapshot-interactive" 1 >/dev/null
  jt_assert_false "$(jt_runtime_eval '!!globalThis.__jellyRuntimeV1')" "second rollback must dispose reinstalled runtime"
}

jt_register "RUN-001" "runtime" "Runtime ref stability" "Verify page-runtime refs are stable for the same node, survive unrelated mutations, and never write Jelly ref attributes into the page." "browser-perf.html small fixture; page runtime enabled." "Repeated snapshots plus unrelated DOM insertion." "Same target keeps the same tokenized ref and document has zero data-jelly-ref attributes." "browser" runtime_ref_stability
jt_register "RUN-002" "runtime" "Live state, visibility and geometry" "Verify disabled, display, opacity, viewport and geometry are recomputed live without ref churn." "browser-perf.html small fixture with a known button." "Mutate disabled/display/opacity/layout styles between snapshots." "State and geometry reflect current rendering; display:none and opacity:0 are excluded; restored node keeps its ref." "browser" runtime_live_state_visibility
jt_register "RUN-003" "runtime" "Replacement and navigation staleness" "Verify node replacement and document navigation create fresh identities and reject stale refs." "browser-perf.html small fixture; runtime ref established." "Replace target node, then navigate to a fresh fixture document." "Replacement/navigation refs differ and old refs fail resolution." "browser" runtime_replacement_navigation
jt_register "RUN-004" "runtime" "Dynamic Shadow DOM invalidation" "Verify late and nested open shadow roots are indexed after runtime installation." "Page runtime installed before attaching a new shadow root." "Attach late open shadow root; load nested-shadow fixture case." "Late and nested shadow controls are searchable exactly once." "browser" runtime_shadow_invalidation
jt_register "RUN-005" "runtime" "Mutation batching and rebuild" "Verify synchronous mutation bursts coalesce while dirty and the next query rebuilds once." "browser-perf.html mutation fixture with clean runtime index." "Append 120 buttons and modify 80 labels in one JS turn." "Epoch advances once, dirty becomes true, one subsequent snapshot clears dirty and indexes new controls." "browser" runtime_mutation_batching
jt_register "RUN-006" "runtime" "Semantic-name invalidation" "Verify ARIA names and value-derived names update after events and silent property changes." "Small fixture with runtime index installed." "Change aria-label; change input values with and without input events." "New names are searchable and stale value-derived names disappear." "browser" runtime_semantic_name_updates
jt_register "RUN-007" "runtime" "Runtime rollback and reinstall" "Verify disabling page runtime restores legacy refs/native hooks and later re-enabling creates a fresh clean runtime." "Small fixture; runtime initially enabled." "Snapshot runtime → legacy → runtime → legacy in same document." "Legacy mode disposes runtime and uses DOM refs; re-enabled runtime uses a fresh token and cleans legacy state." "browser" runtime_rollback_reinstall
