#!/usr/bin/env bash

refs_prepare() {
  jt_with_browser_cleanup
  jt_open_fixture browser-perf.html
  jt_runtime_eval "window.__jellyBench.setCase('small')" >/dev/null
}

refs_token_namespace() {
  refs_prepare
  local first ref token ref_token
  first="$(jt_snapshot 100 0)"
  ref="$(jt_ref_for_name 'small action 3' <<<"$first")"
  jt_assert_nonempty "$ref" "runtime ref missing"
  [[ ! "$ref" =~ ^@e[0-9]+$ && "$ref" == *-* ]] || jt_fail "runtime ref must be document-tokenized: $ref"
  token="$(jt_runtime_eval 'globalThis.__jellyRuntimeV1.documentToken' | tr -d '"')"
  ref_token="${ref#@}"
  jt_assert_eq "${ref_token%%-*}" "$token" "ref token must equal active document token"
  jt_assert_eq "$(jt_runtime_eval "document.querySelectorAll('[data-jelly-ref]').length")" "0" "optimized runtime must not create DOM-backed refs"
}

refs_node_identity() {
  refs_prepare
  local first stable moved replacement
  first="$(jt_snapshot 100 0)"
  stable="$(jt_ref_for_name 'small action 3' <<<"$first")"
  jt_runtime_eval "(()=>{const app=document.querySelector('#app');const b=[...app.querySelectorAll('button')].find(x=>x.textContent==='small action 3');const box=document.createElement('section');box.id='moved-box';app.append(box);box.append(b);return true})()" >/dev/null
  moved="$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 100 0)")"
  jt_assert_eq "$moved" "$stable" "moving the same node must preserve ref"
  jt_runtime_eval "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent==='small action 3');const clone=b.cloneNode(true);b.replaceWith(clone);return true})()" >/dev/null
  replacement="$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 100 0)")"
  jt_assert_ne "$replacement" "$stable" "replacement node must receive distinct ref"
  jt_expect_failure "replaced node old ref must be stale" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" "$stable"
}

refs_collision() {
  refs_prepare
  local first stable raw
  first="$(jt_snapshot 100 0)"
  stable="$(jt_ref_for_name 'small action 3' <<<"$first")"
  raw="${stable#@}"
  jt_runtime_eval "(()=>{const b=[...document.querySelectorAll('button')].find(x=>x.textContent==='small action 3');const c=b.cloneNode(true);b.replaceWith(c);const decoy=document.createElement('button');decoy.textContent='runtime ref decoy';decoy.setAttribute('data-jelly-ref','${raw}');document.body.append(decoy);return true})()" >/dev/null
  jt_assert_eq "$(jt_runtime_eval "document.querySelectorAll('[data-jelly-ref=\"${raw}\"]').length")" "1" "collision decoy must exist"
  jt_expect_failure "tokenized stale runtime ref must never fall through to DOM collision" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" "$stable"
}

refs_image_refs() {
  jt_with_browser_cleanup
  jt_open_fixture delayed-image.html
  local images='' ref info
  for _ in {1..40}; do
    images="$("$JELLY_BIN_DIR/agent-inspect-images")"
    jq -e 'length > 0' >/dev/null <<<"$images" && break
    sleep 0.05
  done
  jt_assert_gt "$(jq 'length' <<<"$images")" "0" "delayed image must become inspectable"
  ref="$(jq -r '.[0].ref // empty' <<<"$images")"
  jt_assert_eq "$ref" "@img1" "inspect-images must return the first DOM-backed image ref"
  info="$(JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" "$ref")"
  jt_assert_eq "$(jq -r '.tag' <<<"$info")" "img" "inspect-images ref must resolve as a normal Jelly target"
}

refs_rollback_navigation() {
  refs_prepare
  local runtime_ref legacy legacy_ref reinstalled reinstalled_ref fixture navigated_ref
  runtime_ref="$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 100 0)")"
  legacy="$(JELLY_PAGE_RUNTIME=0 "$JELLY_BIN_DIR/agent-snapshot-interactive" 10)"
  legacy_ref="$(jq -r '.[0].ref' <<<"$legacy")"
  [[ "$legacy_ref" =~ ^@e[0-9]+$ ]] || jt_fail "legacy ref must use numeric @eN format"
  JELLY_PAGE_RUNTIME=0 "$JELLY_BIN_DIR/agent-element-info" "$legacy_ref" >/dev/null || jt_fail "legacy DOM ref must resolve"
  reinstalled="$(jt_snapshot 100 0)"
  reinstalled_ref="$(jt_ref_for_name 'small action 3' <<<"$reinstalled")"
  jt_assert_ne "$reinstalled_ref" "$runtime_ref" "runtime reinstall must create fresh namespace"
  jt_expect_failure "pre-reinstall runtime ref must be stale" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" "$runtime_ref"
  fixture="file://${JELLY_REPO_ROOT}/tests/fixtures/browser-perf.html"
  "$JELLY_BIN_DIR/agent-navigate" "$fixture" >/dev/null
  jt_runtime_eval "window.__jellyBench.setCase('small')" >/dev/null
  navigated_ref="$(jt_ref_for_name 'small action 3' <<<"$(jt_snapshot 100 0)")"
  jt_assert_ne "$navigated_ref" "$reinstalled_ref" "navigation must create fresh namespace"
  jt_expect_failure "pre-navigation runtime ref must be stale" env JELLY_PAGE_RUNTIME=1 "$JELLY_BIN_DIR/agent-element-info" "$reinstalled_ref"
}

jt_register "REF-001" "refs" "Document-tokenized refs" "Verify optimized refs carry the active document token and do not write page attributes." "browser-perf small fixture with runtime enabled." "Snapshot known target and inspect runtime token/DOM." "Ref is @<documentToken>-N and no data-jelly-ref attributes exist." "browser" refs_token_namespace
jt_register "REF-002" "refs" "DOM identity semantics" "Verify moving the same node preserves identity while replacing it with identical markup creates a new identity." "Known runtime-ref target in small fixture." "Move target node, then replace it with cloneNode(true)." "Moved node keeps ref; replacement gets new ref; original ref becomes stale." "browser" refs_node_identity
jt_register "REF-003" "refs" "Stale-ref collision resistance" "Verify a page-authored data-jelly-ref cannot hijack a stale tokenized runtime ref." "Runtime ref established, then target replaced." "Create DOM element whose data-jelly-ref equals stale runtime token." "Stale tokenized ref fails instead of resolving collision element." "browser" refs_collision
jt_register "REF-004" "refs" "Legacy rollback and document transitions" "Verify runtime→legacy→runtime transitions and navigation maintain separate ref namespaces." "Small fixture with runtime enabled." "Toggle JELLY_PAGE_RUNTIME off/on and navigate to fresh document." "Legacy refs resolve only in legacy mode; each runtime installation/document gets a fresh token and stale refs fail." "browser" refs_rollback_navigation
jt_register "REF-005" "refs" "Image inspection refs are targetable" "Verify DOM-backed refs returned by inspect-images can be reused by ordinary target-taking primitives." "delayed-image.html fixture with one image element." "Run inspect-images, then element-info with its @img1 ref." "The image ref resolves to the same img element through the normal Target resolver." "browser" refs_image_refs
