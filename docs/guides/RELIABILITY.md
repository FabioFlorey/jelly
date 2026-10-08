<div id="top"></div>

# Reliability

**Audience:** Developers designing reliable browser automation and diagnosing workflow failures.

Jelly treats successful execution and verified state as different facts.

An action such as `navigate`, `click`, or `screenshot` reports what Jelly performed or observed. A dependent workflow should continue only when the facts it needs have been established explicitly.

## Evidence and transitions

There is no required linear workflow such as open → inspect → act → capture. Routines are guarded directed graphs. Nodes may be skipped, revisited, or reached from multiple paths.

The invariant is:

> A transition that depends on a fact must be justified by current evidence for that fact.

Typical evidence comes from inspection or verification primitives:

```text
assert-url
assert-title
assert-visible
assert-text
assert-image-ready
wait-for
```

`wait-for` is bounded and supports `exists`, `visible`, `hidden`, `text`, `url`, `title`, and `image-ready`.

Image readiness requires the element to exist, be visible, report `complete == true`, and have nonzero `naturalWidth` and `naturalHeight`.

Jelly element references are page observations, not permanent object IDs. The default page runtime returns document-scoped refs such as `@eabc123-7`; legacy rollback mode uses numeric refs such as `@e7`. `inspect-images` also returns DOM-backed image refs such as `@img2`. If a referenced element disappears after navigation or rerendering, Jelly reports `target_stale`; inspect again before continuing.

## Logical browser targets

Browser pages have runtime logical identities separate from DOM element targets. The primary page is `main`; additional live page targets receive monotonic labels such as `tab-2`, `tab-3`, and so on. Labels are preserved across Jelly processes for the lifetime of the browser runtime, so MCP and CLI calls can refer to the same tab without exchanging Chromium target IDs.

The runtime mapping is reconciled against `Target.getTargets` and stored under `/data/jelly-runtime/state/logical_targets.json`. Chromium `sessionId` values are never persisted. Closed target labels are removed and are not recycled within the same browser runtime. `tabs` surfaces the logical label first and retains the low-level target ID only as diagnostic information. `switch-tab` resolves an exact logical label before its legacy target-ID/title/URL compatibility matching.

`browser-call` may set a per-call `target` for Jelly semantic operations or target-scoped raw CDP. Omitting it preserves active-target behavior. Browser-scoped CDP does not accept a page target because it is sent on the browser connection rather than an attached page session. The small-surface MCP mode is the process default, but this does not weaken Jelly's semantic reliability contract: the same typed errors, ref semantics, verification primitives, and retry discipline apply whether a semantic operation is dispatched directly through large-surface or through small-surface `browser-call`.

Raw CDP has a different reliability contract from Jelly semantic operations. Jelly preflights its structure/routing and reports protocol failures as typed `cdp_failed`, but a successful raw protocol response is not semantic evidence that a workflow goal was achieved. Raw commands may bypass Jelly refs, interaction conventions, verification abstractions, and future policy guards. Keep assertions and verification explicit after raw mutations, and treat browser-scoped raw commands as higher-authority operations than page JavaScript.

## Browser event subscriptions

The small-surface Agent API includes a runtime-scoped `browser-events` builtin with `subscribe`, `poll`, and `unsubscribe` actions. A subscription starts at the current stream tail rather than replaying older retained notifications. Filters support one optional logical target plus exact CDP notification methods and/or method prefixes. Regex/full-message filtering is intentionally not part of the initial contract.

Polling is cursor-based. The cursor contains a stream generation and sequence number; it advances across all observed notifications, including non-matching events, so filters cannot cause the same unrelated frames to be rescanned indefinitely. Poll results expose `cursor_lost`, the number of new ring drops, and the number of stream resets since the previous poll. Ring loss accounting distinguishes a dropped sequence that occurred after the subscription cursor from eviction of history the subscriber had already consumed.

`poll` issues a harmless browser-scoped `Target.getTargets` barrier to drain pending websocket notifications through the synchronous receive loop before reading the ring. There is no background reader. This is a measured design choice for the current poll-based API: deterministic Chromium tests recover a complete 64-event idle burst with no loss, while a 1,500-event idle burst produces exactly the expected 476 evictions from the 1,024-entry ring and exposes that loss through `cursor_lost`/drop accounting. Notifications are therefore observable when a later request or poll drains the socket, not continuously while Jelly is idle. Subscriptions live only in the owning `BrowserSession`; browser/MCP-session restart invalidates their opaque IDs and later use returns `subscription_not_found`. MCP `browser-events` therefore requires persistent MCP browser sessions and is incompatible with `[mcp].persistent_session = false`.

`browser-events` does not implicitly enable CDP domains. Events such as Runtime or Network notifications must already be enabled by the workflow when the protocol requires it. Jelly does enable Target discovery and flattened page-only auto-attach so notifications from multiple page sessions can be attributed to logical targets. The agent addresses `main`/`tab-N`; it never supplies or receives CDP `sessionId` values. Detach/crash clears the affected in-memory session binding, and destroy removes the logical target after retaining the lifecycle event with its logical attribution.

## Errors

Reliability-sensitive browser errors carry stable machine-readable kinds:

```text
invalid_arguments
browser_unavailable
navigation_failed
navigation_timeout
target_not_found
target_not_visible
target_stale
interaction_failed
condition_failed
condition_timeout
javascript_failed
artifact_failed
download_failed
delivery_failed
human_intervention_required
authentication_required
unsupported
subscription_not_found
cdp_failed
internal
```

The message is for people. The kind is for branching. Protocol failures use `cdp_failed` and may also include machine-readable `error.details` with the CDP method, numeric code, and protocol message; transport/session loss remains `browser_unavailable` and retryable. `subscription_not_found` means a browser-event subscription ID is unknown in the current BrowserSession, including after runtime/session restart; repeating the same poll cannot recreate it, so the caller must subscribe again.

Retries are not automatically applied to side-effecting actions. Repeating inspection or verification may be safe; repeating a purchase, submission, message, or destructive click may not be.

## MCP results

Every MCP tool returns a top-level object in `structuredContent`:

```json
{
  "ok": true,
  "data": {},
  "error": null,
  "meta": {
    "tool": "read-page"
  }
}
```

Failures use the same shape:

```json
{
  "ok": false,
  "data": null,
  "error": {
    "kind": "target_stale",
    "message": "stable target @eabc123-4 is no longer present; inspect the page again",
    "retryable": true
  },
  "meta": {
    "tool": "click"
  }
}
```

`data` may naturally contain an object, array, scalar, or null; the outer MCP shape does not change. `error.details` is optional. For a CDP protocol failure it has the stable core shape `{ "protocol": "cdp", "method": "Domain.command", "code": -32601, "message": "..." }`.

## Artifacts

Screenshots are registered artifacts rather than anonymous files. Metadata is stored under:

```text
/data/jelly-runtime/artifacts/metadata/
```

A screenshot record includes its artifact ID, path, creation time, source URL/title, optional target, byte size, image dimensions, trace ID, and verification state.

Capture establishes file integrity, not semantic correctness. A guarded routine may subsequently mark the artifact as captured after named predicates were verified.

For example, `verified-screenshot` records evidence such as `url`, `target-visible`, and `image-ready` when applicable. Captures and registered downloads are archived to immutable Jelly-managed paths before metadata is written. Use an artifact ID in later workflow steps instead of assuming `latest.png` or the newest download is the intended file.

Chromium download lifecycle is tracked from browser-level `Browser.downloadWillBegin` and `Browser.downloadProgress` events. Jelly persists a bounded set of lifecycle records under runtime state with the Chromium GUID, sanitized source URL, suggested filename, received/total byte counts, timestamps, terminal state, optional materialized destination, failure reason, and registered artifact. Terminal states are `completed`, `canceled`, or `interrupted`. Chromium's Browser-domain progress event does not provide a detailed network failure string, so Jelly records bounded operational reasons it can establish itself, such as `canceled_by_user`, `canceled_by_browser`, browser stop/restart or launcher-process exit before completion, or download-tracker disconnection.

`download wait` treats lifecycle state as authoritative and registers a completed managed file as an immutable artifact. An optional destination copies the suggested filename out of Jelly's managed download directory; `fail` and `uniquify` reserve the destination with create-new semantics so a concurrent file cannot be overwritten, while `overwrite` is explicitly destructive. The legacy `wait-download` timestamp/name contract remains supported: when a matching lifecycle record exists it waits on that state; only when no matching record exists does it use the historical filesystem heuristic (temporary-extension filtering, nonzero size, and stable size across the compatibility probe interval).

`verified-download` still records a timestamp immediately before the trigger and uses `wait-download`, but matching Jelly-managed browser downloads now complete from the first-class lifecycle record before artifact verification. The filesystem fallback remains for legacy or externally created files that have no lifecycle event.

## Graph routines

JSON graph routines live beside legacy routines in `.agent/routines/`. A graph declares an `entry` node and a `nodes` object. Nodes can execute tools, evaluate guards, jump directly, suspend for HITL, or terminate.

For a complete, editable workflow definition with executable commands, see [Graph routines](./ROUTINES.md#graph-routines). That guide is the canonical source of routine examples; this section specifies the behavior relevant to recovery and verification.

Cycles are valid. `max_steps` bounds total node executions, `max_visits` can bound a particular recovery node, and `max_duration_ms` bounds wall-clock execution for one run (default: 300000 ms).

Tool output saved with `save` becomes workflow context. Nested values can be referenced with paths such as `{{ artifact.artifact_id }}`. Guards currently support `exists`, `true`, `false`, `equals`, `not_equals`, and `error_kind`.

When a graph successfully opens Chromium, the graph owns that browser. Terminal success, terminal failure, and unhandled execution errors close an owned browser unless `keep_browser` is explicitly true. HITL suspension does not finalize the browser because the routine may need the same session when resumed.

## HITL and challenges

Unexpected login state, CAPTCHA, or another human-only boundary should stop dependent automation and enter HITL rather than trigger speculative clicking.

After human intervention, a graph should normally return to an inspection or verification node. It should not assume the page is still in the state that existed before suspension.

Telegram is the current HITL transport. Jelly requires the Telegram API response to report `ok: true` and include a message ID before delivery is reported as successful.

## Timeouts

Waiting operations have finite deadlines. A timeout is evidence that the requested predicate was not established in time, not evidence that the opposite predicate is permanently true.

Navigation and condition waits therefore return typed timeout errors that a graph can recover from, escalate, or terminate on. CDP socket reads/writes are also bounded by `[browser].cdp_timeout_secs` (default: 60 seconds), so a wedged browser connection does not wait forever.

## Profile reuse

`profile-import` copies a closed Chromium user-data directory into Jelly-managed runtime state:

```bash
cargo run --quiet --bin agent-run -- \
  profile-import /path/to/chromium/user-data
```

Use `--force` to replace an existing Jelly profile. Jelly refuses the import while its browser is running and skips Chromium singleton/DevTools lock files.

Imported profile data may preserve cookies, logins, local storage, and preferences. It is sensitive data and remains outside the repository. Profile reuse does not guarantee CAPTCHA avoidance, fingerprint equivalence, or anti-bot behavior.

## Tracing

Primitive traces record the same typed error kinds used by MCP and graph transitions. Trace context uses `JELLY_TRACE_ID`, `JELLY_PARENT_SPAN_ID`, and `JELLY_SOURCE`.

Artifact records keep the trace ID when available, linking a captured file to the execution that produced it.

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
