<div id="top"></div>

# Routines

Primitives stay small. Routines compose them while preserving browser/session context.

Routine files live in `.agent/routines/`.

## Graph routines

JSON routines are guarded workflow graphs, not fixed pipelines. Nodes can loop, skip work, jump across the graph, suspend for a human, or terminate.

```json
{
  "entry": "inspect",
  "max_steps": 30,
  "nodes": {
    "inspect": {
      "tool": "assert-visible",
      "args": ["{{ target }}"],
      "save": "target_state",
      "next": "capture",
      "on_error": {
        "target_not_found": "recover",
        "*": "fail"
      }
    },
    "recover": {
      "tool": "scroll",
      "args": ["down"],
      "max_visits": 3,
      "next": "inspect"
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

Run it with:

```bash
cargo run --quiet --bin agent-run -- \
  call-routine <name> target=css:.result
```

`save` stores parsed tool output in routine context. Nested values are addressable as `{{ artifact.artifact_id }}`. `on_error` can point to one node or map typed error kinds to different nodes.

Guard nodes use `guard`, `then`, and `else`. Supported operations are `exists`, `true`, `false`, `equals`, `not_equals`, and `error_kind`.

Cycles are valid. `max_steps` limits total node executions, `max_visits` can bound one node, and `max_duration_ms` limits wall-clock execution for one run.

A successful `open-browser` makes the graph the browser owner. Owned browsers are closed on terminal completion or unhandled failure unless `keep_browser` is true. HITL suspension preserves the browser for continuation.

## HITL

A graph HITL node sends the request, verifies delivery, persists graph position/context, then returns a resume ID.

```json
{
  "human": {
    "hitl": "Complete the challenge, then resume",
    "resume": "inspect"
  }
}
```

Resume with:

```bash
cargo run --quiet --bin agent-run -- \
  call-routine resume <id>
```

After HITL, point back to inspection/verification when browser state may have changed.

## Reference routine

`verified-screenshot.json` verifies URL and target state, waits when appropriate, checks image readiness for image targets, captures an artifact, verifies its integrity, records the established predicates, and finalizes its owned browser.

```bash
cargo run --quiet --bin agent-run -- \
  call-routine verified-screenshot \
  url=https://example.com \
  target=css:img
```

`verified-download.json` verifies the page and trigger, records a timestamp baseline, performs the click, waits for a completed non-temporary download newer than that baseline, registers an immutable artifact copy, and verifies it before returning.

```bash
cargo run --quiet --bin agent-run -- \
  call-routine verified-download \
  url=https://example.com/downloads \
  target=css:a.download
```

## Legacy routines

Existing `.jinja` line routines remain supported for simple sequential workflows, signals, handoffs, variables, and HITL. New reliability-sensitive workflows should use JSON graphs.

See [Reliability](./RELIABILITY.md) for transition, error, artifact, timeout, and cleanup semantics.

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
