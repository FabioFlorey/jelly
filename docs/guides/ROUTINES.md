<div id="top"></div>

# Routines

**Audience:** Developers defining reusable browser workflows.

Routines compose focused browser primitives while preserving browser/session context.

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

**Prerequisites:** Chromium is available. Save the JSON above as `.agent/routines/inspect-capture.json` (the filename is the routine name). The following commands change the shared browser page, so use a dedicated Jelly installation or wait until other browser workflows have finished:

```bash
cargo run --quiet --bin agent-run -- open-browser https://example.com
cargo run --quiet --bin agent-run -- \
  call-routine inspect-capture target=css:body
```

**Expected result:** The routine checks the target, captures an artifact, and reaches the `success` terminal node. If inspection fails, its error path may retry scrolling up to the node visit limit before returning a failure. The graph is an example, not a substitute for verifying the required target on the actual page.

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

Resume after the HITL tool returns its resume ID:

```bash
read -r -p "HITL resume ID: " RESUME_ID
cargo run --quiet --bin agent-run -- \
  call-routine resume "$RESUME_ID"
```

**Expected result:** The routine resumes at its recorded node. If the browser or resume state has expired, the command reports an error; do not assume the previous browser state is still present.

After HITL, point back to inspection/verification when browser state may have changed.

## Reference routine

`verified-screenshot.json`, included in `.agent/routines/`, opens Chromium, verifies URL and target state, waits when appropriate, checks image readiness for image targets, captures an artifact, verifies its integrity, and finalizes its owned browser. It is the preferred first smoke test because no separate routine file must be created.

```bash
cargo run --quiet --bin agent-run -- \
  call-routine verified-screenshot \
  url=https://example.com/ \
  target=css:body
```

The trailing slash in `https://example.com/` matters: `verified-screenshot` checks the current URL for an **exact** match.

**Expected result:** The built-in routine ends with `verified screenshot captured` and a registered screenshot artifact. If the URL or element assertion fails, the routine reports a failure instead of treating a screenshot as proof that the page was correct.

`verified-download.json` verifies the page and trigger, records a timestamp baseline, performs the click, then uses the compatibility `wait-download` step. For Jelly-managed Chromium downloads, that wait now prefers the matching first-class CDP lifecycle record and registers its managed file as an immutable artifact; the historical filesystem completion heuristic remains only as a fallback when no matching lifecycle record exists. The routine then verifies the artifact before returning.

```bash
cargo run --quiet --bin agent-run -- \
  call-routine verified-download \
  url=https://your-download-site.example/downloads \
  target=css:a.download
```

The download command is an **illustrative template**, not a runnable public test fixture. Replace the example URL and selector with a real page whose link starts a download before executing it. The routine returns a failure if the download or artifact verification fails.

## Legacy routines

Existing `.jinja` line routines remain supported for simple sequential workflows, signals, handoffs, variables, and HITL. New reliability-sensitive workflows should use JSON graphs.

See [Reliability](./RELIABILITY.md) for transition, error, artifact, timeout, and cleanup semantics.

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
