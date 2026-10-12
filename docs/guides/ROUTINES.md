<div id="top"></div>

# Reusable routines

## Table of Contents

- [Purpose: save a verified workflow](#purpose-save-a-verified-workflow)
- [Execution modes and agent handoffs](#execution-modes-and-agent-handoffs)
- [Discover, generate, and save routines](#discover-generate-and-save-routines)
- [JSON graph format](#json-graph-format)
- [Variables, conditions, and loops](#variables-conditions-and-loops)
- [Chain routines](#chain-routines)
- [Human approval and resumption](#human-approval-and-resumption)
- [Included examples](#included-examples)
- [Safety and limitations](#safety-and-limitations)

**Audience:** People and AI agents creating repeatable browser workflows, and developers reviewing their execution guarantees.

## Purpose: save a verified workflow

Sometimes an agent completes a workflow that is already **airtight**: it knows which actions to take, how to check each result, and what to do when something fails. Rather than rediscovering those steps every time, Jelly can save the workflow as a **named JSON routine**.

A routine has a **name** and **description** so an agent can find it later. It accepts **variables** for the parts that change between runs. It can **branch on conditions**, **loop with explicit budgets**, and **call other saved routines** to build larger workflows.

Routines are not a substitute for verification. Before saving one, the AI should establish that the workflow actually works in the intended environment. Saving a graph is not proof of its correctness, and running one still checks actual browser state.

Use a normal `browser-call` batch for a simple one-off sequence. Use a routine when the workflow is worth *remembering and reusing*.

## Execution modes and agent handoffs

Jelly uses **one JSON graph runner** with three execution modes. A definition that
does not set `mode` retains the legacy `auto` behavior.

| Mode | Graph execution | Agent participation |
| --- | --- | --- |
| `auto` | Browser/system tools, conditions, bounded loops, routine calls | None; agent nodes are rejected |
| `hybrid` | Deterministic by default; `on_error` can branch to an agent node | Explicit fallback when a graph routes to an agent node |
| `jelly` | Graphs can include agent nodes in their normal path | Each agent node suspends execution until an external agent acts |

A **macro-routine** is composition, not a fourth runner: any graph can call
saved routines using `call-routine` and pass selected variables to each child.

An `agent` node is a **structured handoff**, not an embedded LLM call. It renders
a text instruction using routine variables and returns a suspended result with
`reason: "agent_required"`, `instruction`, `node`, `verify`, `resume_id`, and,
when present, the typed `_error` from the failed operation. Jelly persists the
graph and its context. The calling agent then uses Jelly's ordinary browser tools
to resolve the situation and calls `call-routine` with `resume_id` to continue.
**No model runs inside Jelly, and the continuation is not automatic.**

Example hybrid fallback:

```json
{
  "name": "invoice-download",
  "description": "Find and download an invoice; recover a changed button with the calling agent.",
  "mode": "hybrid",
  "entry": "click",
  "max_steps": 20,
  "nodes": {
    "click": {
      "tool": "click",
      "args": ["text:Download invoice"],
      "next": "verify",
      "on_error": {"target_not_found": "repair"}
    },
    "repair": {
      "agent": "Find and click the download button for invoice {{ invoice }}.",
      "resume": "verify"
    },
    "verify": {
      "tool": "wait-for",
      "args": ["text", "Download complete", "10"],
      "next": "done"
    },
    "done": {"terminal": "success"}
  }
}
```

An agent node is also valid in `mode: "jelly"` and can be the graph entry.
`resume` **must point directly to a supported verification tool**, such as
`assert-visible`, `assert-url`, `assert-text`, `assert-title`, `assert-image-ready`,
`wait-for`, `verify-artifact`, or `wait-download`. This prevents silently
advancing past a delegated action without a verification attempt. The
verification can still fail, and success is not inferred from the agent's claim.
The example's condition is illustrative; adapt verification to observable
website behavior.

The existing `hitl` node remains separate: it requests human intervention
via Telegram. An agent node yields work back to the caller through the routine
result. Neither mechanism is a general durable background agent scheduler.

## Discover, generate, and save routines

The `call-routine` MCP tool lets an AI client **list**, **describe**, **save**, **run**, and **resume** JSON routines. To generate one, the agent constructs a graph from a previously verified workflow, gives it a meaningful name and description, and invokes `call-routine` with `action: "save"` and a `graph` object.

Jelly validates the graph structure and saves it as `.agent/routines/<name>.json`. The save operation **never executes** the workflow or overwrites an existing routine. Newly saved definitions receive a generated **UUID v4** (`id`) if one was not provided. Existing definitions without an ID remain valid; they can still be addressed by name.

To inspect saved routines from the CLI:

```bash
cargo run --quiet --bin agent-run -- call-routine list
cargo run --quiet --bin agent-run -- call-routine --search screenshot
cargo run --quiet --bin agent-run -- call-routine describe verified-screenshot
```

An AI client can use this MCP request shape to save its generated graph:

```json
{
  "name": "call-routine",
  "arguments": {
    "action": "save",
    "graph": {
      "name": "check-page",
      "description": "Check that a requested target is visible on the current page.",
      "entry": "check",
      "max_steps": 10,
      "nodes": {
        "check": {
          "tool": "assert-visible",
          "args": ["{{ target }}"],
          "save": "evidence",
          "next": "done",
          "on_error": "failed"
        },
        "done": {"terminal": "success", "message": "Target confirmed"},
        "failed": {"terminal": "failure", "message": "Target not confirmed"}
      }
    }
  }
}
```

This is the `tools/call` **arguments envelope**, not a full HTTP JSON-RPC message. The resulting routine can be called with `name: "check-page"` and `vars: {"target": "css:main"}`.

To save a graph already written to a local JSON file:

```bash
cargo run --quiet --bin agent-run -- call-routine save "$(cat my-workflow.json)"
```

Saved routine names use lowercase letters, digits, hyphens, and underscores, starting with a letter. Descriptions are required. `list` and `--search` include the execution mode and UUID if present. `--search` performs a **case-insensitive substring match** on the name and description, not semantic/embedding search. `describe` and execution accept either a name or a UUID from `list`. Use `describe` to review the actual saved graph before running it.

## JSON graph format

Every saved routine is **JSON**, with `name`, `description`, `entry`, and `nodes`. Each node represents a browser/system tool, an explicit decision, a jump, a human request, or a terminal outcome.

```json
{
  "name": "inspect-capture",
  "description": "Capture a verified screenshot after finding a target.",
  "entry": "inspect",
  "max_steps": 30,
  "nodes": {
    "inspect": {
      "tool": "assert-visible",
      "args": ["{{ target }}"],
      "save": "target_state",
      "next": "capture",
      "on_error": {"target_not_found": "recover", "*": "fail"}
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
    "done": {"terminal": "success"},
    "fail": {"terminal": "failure"}
  }
}
```

Here, `inspect` verifies the element. If it cannot be found, Jelly scrolls and retries, with a bounded number of visits. A successful check leads to screenshot capture; a failed check does **not** count as a successful screenshot.

## Variables, conditions, and loops

Pass values when calling a routine:

```bash
cargo run --quiet --bin agent-run -- call-routine inspect-capture target=css:body
```

`{{ target }}` is a **JSON-graph context placeholder**, not a Jinja template file. Jelly resolves it from the call's variables; `save` stores parsed results in the same context. Nested values such as `{{ artifact.artifact_id }}` are available to later nodes.

A condition uses `guard`, `then`, and `else`:

```json
{
  "guard": {"path": "target_state.visible", "op": "true"},
  "then": "capture",
  "else": "inspect"
}
```

Supported guard operations are `exists`, `true`, `false`, `equals`, `not_equals`, and `error_kind`. Loops are ordinary graph edges; limit them with `max_steps`, optional node-level `max_visits`, and `max_duration_ms` so a routine cannot retry indefinitely.

## Chain routines

A routine can invoke another saved routine using the same `call-routine` tool. Pass variables as `key=value` strings and save the returned result for later guards.

```json
{
  "name": "capture-two-pages",
  "description": "Verify and capture screenshots of two requested pages.",
  "entry": "first",
  "max_steps": 6,
  "nodes": {
    "first": {
      "tool": "call-routine",
      "args": ["verified-screenshot", "url={{ first_url }}", "target=css:body"],
      "save": "first_result",
      "next": "second",
      "on_error": "fail"
    },
    "second": {
      "tool": "call-routine",
      "args": ["verified-screenshot", "url={{ second_url }}", "target=css:body"],
      "save": "second_result",
      "next": "done",
      "on_error": "fail"
    },
    "done": {"terminal": "success", "message": "Both pages verified"},
    "fail": {"terminal": "failure", "message": "One page failed"}
  }
}
```

Call the saved routine with `first_url=https://example.com/` and `second_url=https://example.org/`. Jelly limits nested routine calls to **eight levels**. Each child has its own graph budget, context, and browser ownership; do not assume a child routine shares the parent's in-memory browser session.

## Human approval and resumption

A graph can request human intervention, save its position, and resume after the person completes a required step:

```json
{
  "human": {
    "hitl": "Complete the challenge, then resume",
    "resume": "inspect"
  }
}
```

Use the returned resume ID:

```bash
cargo run --quiet --bin agent-run -- call-routine resume "<resume-id>"
```

A resumed routine must recheck browser state if a person could have changed it.

## Included examples

Jelly includes two runnable graph definitions in `.agent/routines/`:

- **`verified-screenshot`:** Opens Chromium, verifies the URL and target, waits if needed, captures a screenshot, verifies the artifact, and closes its owned browser.
- **`verified-download`:** Verifies a download trigger, clicks it, waits for completion, and verifies the downloaded artifact.

Try the screenshot routine on a dedicated or idle Jelly browser:

```bash
cargo run --quiet --bin agent-run -- \
  call-routine verified-screenshot \
  url=https://example.com/ \
  target=css:body
```

The trailing slash matters because this routine uses an exact URL assertion. The download routine requires a real website with a download link; see the [website examples](https://fabioflorey.github.io/jelly/examples.html#routines).

## Safety and limitations

A saved graph runs **real browser and system operations**. Inspect generated graphs before running them; favor semantic browser operations and explicit verification over arbitrary JavaScript or raw CDP. A save operation never launches a browser, but a later run may navigate or change shared state.

Saving requires write access to the repository's `.agent/routines/` directory. Generated JSON files are ordinary files in that checkout and should be reviewed before committing to version control. Existing names and UUIDs are not silently replaced. Legacy `.jinja` routines are **not supported**; use JSON graphs for all new and existing workflows.

The current implementation does **not** provide autonomous model invocation,
an agent wake-up adapter, routine version history, typed routine input/output
contracts, semantic search, or automatic promotion of agent traces into auto
routines. Those are separate future steps. Agents must inspect the handoff
result and explicitly resume; a `panic` in Jelly is not a supported recovery
trigger. Ensure delegated actions are idempotent or inspect the browser state
before retrying side effects (submissions, downloads, purchases). Suspended
routines retain their captured definition so a later edit to a saved file
does not retroactively change the suspended graph.

See [Reliability](./RELIABILITY.md) for transition, error, artifact, timeout, and cleanup semantics.

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
