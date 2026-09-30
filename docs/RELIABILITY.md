<div id="top"></div>

# Reliability

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
internal
```

The message is for people. The kind is for branching.

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

`data` may naturally contain an object, array, scalar, or null; the outer MCP shape does not change.

## Artifacts

Screenshots are registered artifacts rather than anonymous files. Metadata is stored under:

```text
/data/jelly-runtime/artifacts/metadata/
```

A screenshot record includes its artifact ID, path, creation time, source URL/title, optional target, byte size, image dimensions, trace ID, and verification state.

Capture establishes file integrity, not semantic correctness. A guarded routine may subsequently mark the artifact as captured after named predicates were verified.

For example, `verified-screenshot` records evidence such as `url`, `target-visible`, and `image-ready` when applicable. Captures and registered downloads are archived to immutable Jelly-managed paths before metadata is written. Use an artifact ID in later workflow steps instead of assuming `latest.png` or the newest download is the intended file.

`verified-download` records a timestamp immediately before the download trigger, waits for a completed non-temporary file newer than that baseline, registers it as an artifact, then verifies the artifact before the graph can complete.

## Graph routines

JSON graph routines live beside legacy routines in `.agent/routines/`. A graph declares an `entry` node and a `nodes` object. Nodes can execute tools, evaluate guards, jump directly, suspend for HITL, or terminate.

```json
{
  "entry": "inspect",
  "max_steps": 40,
  "nodes": {
    "inspect": {
      "tool": "assert-visible",
      "args": ["{{ target }}"],
      "save": "target",
      "next": "capture",
      "on_error": {
        "target_not_found": "recover",
        "*": "fail"
      }
    },
    "recover": {
      "tool": "scroll",
      "args": ["down"],
      "next": "inspect",
      "max_visits": 3
    },
    "capture": {
      "tool": "screenshot",
      "args": ["{{ target }}"],
      "save": "artifact",
      "next": "done"
    },
    "done": { "terminal": "success" },
    "fail": { "terminal": "failure" }
  }
}
```

Cycles are valid. `max_steps` bounds total node executions, `max_visits` can bound a particular recovery node, and `max_duration_ms` bounds wall-clock execution for one run (default: 300000 ms).

Tool output saved with `save` becomes workflow context. Nested values can be referenced with paths such as `{{ artifact.artifact_id }}`. Guards currently support `exists`, `true`, `false`, `equals`, `not_equals`, and `error_kind`.

When a graph successfully opens Chromium, the graph owns that browser. Terminal success, terminal failure, and unhandled execution errors close an owned browser unless `keep_browser` is explicitly true. HITL suspension does not finalize the browser because the routine may need the same session when resumed.

## HITL and challenges

Unexpected login state, CAPTCHA, or another human-only boundary should stop dependent automation and enter HITL rather than trigger speculative clicking.

After human intervention, a graph should normally return to an inspection or verification node. It should not assume the page is still in the state that existed before suspension.

Telegram is the current HITL transport. Jelly requires the Telegram API response to report `ok: true` and include a message ID before delivery is reported as successful.

## Timeouts

Waiting operations have finite deadlines. A timeout is evidence that the requested predicate was not established in time, not evidence that the opposite predicate is permanently true.

Navigation and condition waits therefore return typed timeout errors that a graph can recover from, escalate, or terminate on. CDP socket reads/writes are also bounded by `JELLY_CDP_TIMEOUT_SECS` (default: 30 seconds), so a wedged browser connection does not wait forever.

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

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
