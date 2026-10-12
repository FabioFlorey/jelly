<div id="top"></div>

# Reusable routines

## Table of Contents

- [Purpose: save a verified workflow](#purpose-save-a-verified-workflow)
- [Execution modes and agent handoffs](#execution-modes-and-agent-handoffs)
- [Discover, generate, and save routines](#discover-generate-and-save-routines)
- [Immutable revisions and promotion](#immutable-revisions-and-promotion)
- [Routine Capture sessions](#routine-capture-sessions)
- [Trace crystallization into a staged candidate](#trace-crystallization-into-a-staged-candidate)
- [JSON graph format](#json-graph-format)
- [Variables, conditions, and loops](#variables-conditions-and-loops)
- [Optional input/output contracts](#optional-inputoutput-contracts)
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

### Handoff destination: caller by default

**The agent invoking Jelly is the default recipient.** A `call-routine` response
with `status: "suspended"`, `reason: "agent_required"` and
`handoff: {"target":"caller","delivery":"returned",...}` is directed to that
same agent, in the same tool call. The agent should inspect the page, perform
the permitted action, then call `call-routine` with `resume_id`. Completion
is determined by the next verification node, not by an untrusted claim.

The caller may instead select an **operator-configured** profile when starting
a routine (not when resuming):

```json
{"name":"invoice-download","vars":{"invoice":"2026-10"},"handoff_to":"local-worker"}
```

This is the MCP tool *arguments* object. CLI equivalent:

```bash
agent-run call-routine --handoff-to local-worker invoice-download invoice=2026-10
```

Profiles are defined by the operator in `config/jelly.toml`; tools cannot
provide arbitrary command paths or endpoint URLs:

```toml
[[routines.agents]]
name = "local-worker"
transport = "local"
program = "/usr/local/bin/jelly-agent-adapter"
args = []

[[routines.agents]]
name = "remote-worker"
transport = "remote"
url = "https://agents.example.com/jelly/handoff"
bearer_token_env = "JELLY_ROUTINE_AGENT_TOKEN"
```

The local adapter is **an external executable**, not an LLM built into Jelly:
Jelly starts it without a shell, writes one UTF-8 JSON handoff object to stdin,
and waits up to five seconds for exit status 0 (acknowledgement). A remote adapter
receives the same JSON as an HTTPS POST with
`Content-Type: application/json`; an optional bearer token is loaded from the
named environment variable, never stored in a graph. Remote delivery uses
`curl` and a five-second timeout. Both adapters must acknowledge quickly and
manage actual agent execution independently; success in dispatch means only
that the handoff was accepted for delivery.

The JSON includes `resume_id`, `routine`, `node`, `instruction`, `verify`,
and the typed error when present. The adapter must arrange for its agent to
access Jelly with its own authorized MCP connection, perform the task and
call `call-routine` again with `resume_id`. Do not treat the ID as an access
token. Authentication to Jelly remains mandatory. Delivery may expose
instruction text and error details to the selected agent: choose trusted
endpoints, secure environment variables and restrict who may select profiles.

If external delivery fails, Jelly **keeps the checkpoint** and returns a
structured result with `handoff.delivery: "failed"` and `fallback: "caller"`.
The original agent can recover manually. Unknown or malformed named profiles
are rejected *before* executing the routine. The selected destination is
persisted in the checkpoint and cannot be replaced when resuming.

In a macro-routine, a suspended child pauses its parent. The original caller
receives a **parent** `resume_id`. The parent checkpoint retains the child
continuation and resumes it first, then checks its outcome before continuing.
A nested agent handoff is dispatched once, at the outermost routine boundary.
This applies to nested human suspension too, without re-sending the HITL request.

MCP does not guarantee that a caller model will automatically continue after
receiving a tool result; Jelly supplies the explicit instruction and checkpoint.
No persistent worker, automatic wake-up or provider-managed model session is
silently assumed.

### Durable status, completion receipts and safe resumption

A caller can check the continuation status at any time, including after an
external agent has completed it:

```json
{"action":"status","resume_id":"17917883592861916161836"}
```

CLI equivalent: `agent-run call-routine --status <resume-id>`.
This is a **polling interface**: it does not send a push notification to a
disconnected MCP client. Valid statuses are:

| Status | Meaning |
| --- | --- |
| `suspended` | Saved checkpoint awaiting external action or HITL; `awaiting` describes the pending instruction if known |
| `running` | Another process currently holds the continuation lock |
| `claimed` | An agent has acknowledged the handoff and holds an unexpired lease |
| `expired` | The agent's lease elapsed; do not automatically replay its side effects |
| `completed` | The continuation finished and a durable success receipt exists |
| `failed` | The graph reached an explicit failed terminal |
| `interrupted` | The process stopped or erred with an uncertain outcome; do **not** blindly retry |

### Explicit agent acknowledgement and lease

The default behavior remains backwards-compatible: a handoff returns to the
calling agent and can be resumed directly. For asynchronous handoffs, an agent
may **claim** a suspended agent node before working:

```json
{"action":"claim","resume_id":"17917883592861916161836","owner":"local-worker","ttl_seconds":900}
```

`claim` returns `lease_token` (a new 128-bit random value) and `expires_at`.
Keep the token private: it is a bearer capability, **not proof of an
authenticated agent identity**. Owner labels are only attribution metadata.
To renew the lease, call `{"action":"renew","resume_id":"...","lease_token":"..."}`
(with optional `ttl_seconds`: 60-3600, default 900). To release it, call
`{"action":"release","resume_id":"...","lease_token":"..."}`. Only a safely
suspended agent handoff can be claimed; human HITL, running, interrupted, and
terminal states cannot be claimed or renewed.

A claimed routine **requires** the token to resume:

```json
{"resume_id":"17917883592861916161836","lease_token":"<secret>"}
```

CLI equivalents:

```bash
agent-run call-routine --claim "$RESUME_ID" local-worker 900
agent-run call-routine --renew "$RESUME_ID" "$LEASE_TOKEN" 900
agent-run call-routine --release "$RESUME_ID" "$LEASE_TOKEN"
agent-run call-routine resume "$RESUME_ID" --lease "$LEASE_TOKEN"
```

The lease persists across repeated agent suspensions, including parent
macro-routine checkpoints. When the workflow delegates instead to human HITL,
including a suspended human child, Jelly releases the agent lease. `status` exposes owner and expiry but never the
token. A second claim or invalid token is rejected, as is a resume after lease
expiry. **Expiration alone never authorizes another agent to execute actions.**
After verifying potential external side effects, an operator may explicitly
take over using `claim` with `takeover_reason` (12-300 characters), for example:

```json
{"action":"claim","resume_id":"...","owner":"recovery-worker","takeover_reason":"Checked invoice state; no submission occurred"}
```

This rotates the token and invalidates the prior claim. Takeover is not an
automatic safety guarantee: an old agent might still be working outside
Jelly. Applications requiring hard mutual exclusion across independent
agents need additional execution-level coordination. Jelly never starts
an agent merely because a lease expires, and MCP authentication is still
required for every call. Token-bearing arguments and private checkpoint
files deserve the same care as other secrets.

Only one process can hold the advisory lock for a given `resume_id`. Concurrent
OLD
`resume` calls fail without starting a duplicate workflow. After completion,
a repeated `resume` returns the stored result with `replayed: true`, without
executing another action. A newly started `resume` marks the checkpoint as
in-flight *before* running any node: after an unclean process exit, `status`
reports `interrupted` and the default `resume` refuses unsafe replay.
The operator must inspect side effects and arrange an explicit recovery;
automatic recovery from uncertain execution is not implemented.

Checkpoint writes are atomic and owner-readable only. Completion receipts
are also private (mode 0600), and may contain the full workflow output and
variables so duplicate deliveries can retrieve the original result. They
**remain on disk** at the configured runtime directory until the operator
performs retention maintenance; do not put unnecessary secrets into routine
variables. Agents must still authenticate to Jelly; continuation IDs and
advisory locks do not establish cryptographic identity or authorization
ownership of a particular agent.

This provides **single-writer execution and safe duplicate completion lookup**,
not exactly-once guarantees for a website's external side effects. An action
can complete remotely just before an unexpected process crash. Jelly then
reports uncertainty rather than silently replaying that action.

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

## Optional input/output contracts

A saved JSON graph can declare `inputs` and `outputs` without changing the
existing untyped format. Contracts are optional; old routine definitions
and invocations remain valid.

```json
{
  "name": "invoice",
  "description": "Return the requested invoice identifier.",
  "entry": "done",
  "inputs": {
    "invoice_id": {"type": "string", "required": true}
  },
  "outputs": {
    "id": {"from": "invoice_id", "type": "string", "required": true}
  },
  "nodes": {"done": {"terminal": "success"}}
}
```

Inputs passed via CLI or MCP `vars` are strings. Therefore only
`type: "string"` is valid for inputs currently. A required input must
be present before any node executes. Other supplied variables are still
allowed for backward compatibility.

Output specifications use `from` to read a dot-separated path from the
completed graph context; supported output types are `string`, `number`,
`boolean`, `object` and `array`. The `required` flag defaults to false.
A required missing output or a present value with the wrong type fails the
routine before it reports successful completion. When `outputs` is declared,
the completed result includes an `outputs` object in addition to the legacy
`context`, allowing a parent macro-routine to reference
`{{ child.outputs.id }}` after saving the result under `save: "child"`.

The contracts validate data types and presence; they do not verify website
facts. Graph-defined assertions remain responsible for checking real-world
outcomes. Typed inputs other than strings, schema revisions, secret-field
classification and output-only return envelopes are future extensions.

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
a standardized agent wake-up/completion protocol, typed non-string routine
inputs, semantic search, or automatic promotion of agent traces. Agents must
inspect the handoff result and explicitly resume; a `panic` in Jelly is not
a supported recovery trigger. Ensure delegated actions are idempotent or
inspect the browser state before retrying side effects (submissions,
downloads, purchases). Suspended routines retain their captured definition
so a later edit to a saved file does not retroactively change the run.

See [Reliability](./RELIABILITY.md) for transition, error, artifact, timeout, and cleanup semantics.

## Immutable revisions and promotion

New routines are saved at revision 1. Existing JSON routines without a
`revision` field are implicitly revision 1 and remain valid. The active alias
is still `.agent/routines/<name>.json`; immutable historical revisions live at
`.agent/routines/.versions/<name>/vN.json`. When sharing routines through Git,
commit both the active alias and its archived revisions.

Revision management is explicit and never executes a routine. First inspect
versions with `call-routine` action `versions` and the routine `name`. To stage
a candidate, call `revise` with `name`, `expected_revision` and the complete
`graph`; omit `revision` (Jelly assigns it) and keep the same name and UUID.
Staging does **not** change the active alias.

```json
{"action":"versions","name":"my-routine"}
```

```json
{"action":"revise","name":"my-routine","expected_revision":1,
 "graph":{"name":"my-routine","description":"New verified procedure",
          "entry":"done","nodes":{"done":{"terminal":"success"}}}}
```

After testing/reviewing the staged version, explicitly promote it:

```json
{"action":"promote","name":"my-routine","revision":2,"expected_revision":1}
```

Equivalent CLI commands: `call-routine --versions my-routine`,
`call-routine --revise my-routine 1 '<graph-json>'` and
`call-routine --promote my-routine 2 1`. To execute or describe a pinned
revision use `my-routine@v2`; `my-routine` continues to mean the active alias.
Promoting an older revision is an explicit rollback. All archived revisions
remain available, and subsequent staging uses the next unused revision number.

A per-routine filesystem lock serializes revision changes. Staging uses
create-new semantics for immutable archives and promotion atomically replaces
the active alias. `expected_revision` prevents lost updates against a changed
active routine. Existing suspended workflows retain their captured definition.
Versioning does not certify a procedure as safe; reviewing and testing
candidates before promotion is still required.

## Routine Capture sessions

Jelly can now manage a **capture session** across separate CLI commands and
MCP browser primitives. There is no need to set `JELLY_TRACE_ID` manually.
The session stores an automatically generated `capture_id` (also its trace
ID) and records subsequent primitive outcomes in Jelly's existing
`logs/actions.jsonl`. Capture does not open Chromium, run a workflow or start
`record-browser`, and captures no video/screenshots by itself.

Start the session through `call-routine`:

```json
{"action":"capture","operation":"start"}
```

Then use Jelly normally to perform browser operations and explicit assertions.
MCP calls and `agent-run` CLI calls share the same active capture ID. Inspect
progress, then stop it by supplying the ID returned from start:

```json
{"action":"capture","operation":"status"}
```

```json
{"action":"capture","operation":"stop",
 "capture_id":"capture-0123456789abcdef0123456789abcdef"}
```

CLI equivalents:

```bash
agent-run call-routine --capture start
agent-run call-routine --capture status
agent-run call-routine --capture stop "capture-0123456789abcdef0123456789abcdef"
agent-run call-routine --capture status "capture-0123456789abcdef0123456789abcdef"
```

`status` without an ID returns `idle` when no capture is active; with an ID
it inspects a previous session. Status returns primitive and verification
counts, failed event counts, wrapper event counts, and unsupported primitive
counts; it does **not** return raw arguments. Only one capture is active per
Jelly runtime, and the capture metadata is persisted privately under the
runtime's `routines/captures` directory (mode 0700, files 0600). The active
capture survives restarts until explicitly stopped. Start and stop are
serialized across processes using a per-runtime filesystem lock.

**Shared runtime scope:** a capture records actions from *all* agents and CLI
processes using the same Jelly runtime while it is active. It is not yet
isolated to one MCP client. Do not start capture when unrelated clients might
use the browser, and do not run multiple unrelated workflows in the same
capture. Stop after all relevant calls have completed; a command still
in flight at stop time may not be included. The log uses existing argument
redaction; never assume that web text or other recorded values cannot contain
sensitive content.

The resulting `capture_id` can then be provided as `trace_id` to the existing
`crystallize` operation to stage a **candidate**, after first creating or
selecting a named saved routine. Stopping capture never invokes
`crystallize` and never activates a revision. The converter's strict
action/verification allowlist still applies; captured sessions with arbitrary
navigation, form entry or other unsupported events remain available as
diagnostic evidence but cannot automatically become `auto` graphs.

## Trace crystallization into a staged candidate

The `crystallize` management action reconstructs a **candidate auto-routine**
from Jelly's existing `logs/actions.jsonl` trace events. It does **not** run
the candidate, change the current active routine, perform model inference, or
assert that replaying the procedure will succeed.

The agent must have executed browser primitives with a **shared trace ID**
(prefer `capture start`/`stop` above; manually setting `JELLY_TRACE_ID` remains
an alternative). Independently generated trace IDs cannot be merged
automatically. The shared trace contains real `primitive.completed` events
with successful, explicit verification operations after each action. The
caller supplies the active saved routine's `name` and `expected_revision`:

```json
{"action":"crystallize","name":"my-routine","expected_revision":1,
 "trace_id":"trace-example-123"}
```

CLI equivalent:

```bash
cargo run --quiet --bin agent-run -- call-routine \
  --crystallize my-routine 1 trace-example-123
```

Jelly reads **only** the configured `actions.jsonl`; the API does not accept
an arbitrary log file path. The first converter deliberately supports just
`click`, `check`, `select` and `scroll`, each immediately followed by one
successfully observed `assert-visible`, `assert-text`, `assert-title`,
`assert-image-ready` or `wait-for` (no JS/URL predicate). It rejects failed
steps, omitted checks, script execution, form data, navigation, uploads,
ephemeral `@e...` element references, redacted arguments, secrets, unsupported
tools, oversized logs/traces and ambiguous sequences. This intentionally
rejects many legitimate traces instead of inventing a runnable operation.

A passing conversion creates the next **staged, immutable revision**. The
candidate has `mode: "auto"` and a `crystallization` provenance record
including `trace_id`, action/verification counts and `reviewed: false`.
Review `describe my-routine@vN` and the actual log evidence, check that
selectors and inputs generalize, test the proposed revision in a safe
environment, and only then use explicit `promote` with the expected active
revision. For side-effecting actions, a successful check in one run does not
guarantee the action is safe or idempotent in later runs.

The converter does not infer variables, branches, loops, recovery policies,
correctness from screenshots, or an optimal procedure. It requires explicit
recorded assertions, cannot safely reconstruct redacted values, and does not
replace human/agent review. Future work can add scoped capture sessions and
parameterization before candidate staging.

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
