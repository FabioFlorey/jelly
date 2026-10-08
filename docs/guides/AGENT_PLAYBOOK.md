<div id="top"></div>

# Agent Tool-Selection Playbook

**Audience:** Agents and maintainers resolving tool ambiguity while operating Jelly.

The short normative bootstrap is [`.agent/instructions/mcp.md`](../../.agent/instructions/mcp.md). This playbook is intentionally retrievable on demand, not part of every MCP `initialize` response. The currently published `tools/list` and `inputSchema` remain authoritative, followed by `browser-schema` for small-surface semantic operations. Examples show patterns, not an alternative API definition.

## Choose the smallest applicable capability

| User intent | Prefer | Avoid confusing it with |
| --- | --- | --- |
| Learn a tool name or parameter schema | `browser-schema search` then `schema` (small MCP); `agent-discover` (local CLI) | A speculative `click` or `evaluate-js` probe |
| Understand the current page | `read-page` | `browser-events`, which records protocol notifications |
| Find a named control | `find-interactive` | `snapshot-interactive`, which lists a broader inventory |
| List many controls and obtain current refs | `snapshot-interactive` | `find-interactive`, intended for targeted search |
| Replace a form value | `fill` | `type-text`, intended for caret typing/appending |
| Verify an expected post-action state | `assert-text`, `assert-visible`, `assert-url`, `wait-for` | `click` success or an unconditional `wait` |
| Capture one image | `screenshot` | `record-browser` for multi-action video |
| Document a walkthrough | `record-browser` with `mode: steps`; stop then `verify-artifact` | A collection of manually timed screenshots |
| Execute branches, loops, or recoverable stages | `call-routine` | A simple ordered `browser-call` batch |
| Observe dropped CDP notifications | `browser-events` | `read-page` or raw CDP |
| Deliver a user-requested MP4 using Telegram | `hitl` with `video` when no active ChatGPT chat exists | A browser UI click or an unverified send attempt |
| No semantic capability can express the request | Inspect `evaluate-js` or published raw-CDP contracts, with explicit justification | Blindly escalating to the broadest interface |

For small-surface MCP, semantic actions are **named calls inside `browser-call`**. For large-surface MCP, they are individually advertised primitive tools. The CLI uses `agent-run <tool> [args...]`; CLI positional syntax must never be copied into the MCP JSON argument object.

## Example: select an option and verify it

1. If the action contract is unknown, call `browser-schema` with `{"action":"search","query":"find interactive control"}` and then `{"action":"schema","operation":"find-interactive"}`.
2. Use the advertised semantic operation to inspect current controls. Locate the exact intended option from its accessible name or observed element ref.
3. Click or check the intended control using `browser-call` (small surface) or the individual advertised operation (large surface).
4. If the site displays a confirmation dialog, inspect its controls and confirm only when consistent with the user's request.
5. Re-inspect selected state and verify the final outcome, including any new price. A dispatched click is not enough.
6. If `target_stale` occurs, discard the old ref and re-inspect before any retry. If the prior action may have committed a change, inspect before retrying at all.

An example named small-surface call after the `click` schema is known:

```json
{
  "calls": [{"call": {"jelly": "click", "params": {"target": "css:button.apply"}}}],
  "on_error": "stop"
}
```

The target above is illustrative; agents MUST ground targets in current browser observations and MUST verify the result separately.

## Example: a recorded browser demonstration

1. Start the browser and inspect the site. Apply the standard cookie rule: reject optional cookies.
2. Call `record-browser` with `{"action":"start","mode":"steps"}` before the actions to include in the recording.
3. Navigate and interact through semantic Jelly operations. Inspect after any page change; keep captions accurate to actions that truly occurred.
4. Stop with `{"action":"stop"}`. Verify the returned artifact ID using `verify-artifact`; registration or video creation alone does not establish semantic success.
5. Compare the browser's observed selections and final state with the user's requirements. If sending via Telegram was explicitly requested, use `hitl` with the local MP4 path, then check `accepted` and the returned message ID.
6. Report the result and any incomplete steps. Close only Jelly-owned browser state when the user has not requested continued use.

## Recovery and final-output gate

- **Interface unknown:** discover the tool schema, never mutate to test guessed arguments.
- **Page unknown:** inspect the current page or target, not protocol events.
- **Intent unknown:** ask the user; do not guess a purchase, security permission, or other authorization.
- **Action failed or timed out:** inspect for side effects, classify the typed error, and retry only if safe.
- **Verification failed:** do not report the step as complete. Gather fresh evidence, use an alternate legitimate UI path, or report the blocker.
- **Artifact failed integrity check:** do not claim the recorded output is delivered. Re-capture only when safe.
- **MCP/client surface differs:** inspect `tools/list` again; do not assume names from another mode.

Before reporting success: **requested outcome → observed evidence → artifact integrity (if applicable) → delivery acknowledgement (if applicable)**. Every arrow must be supported; distinguish unknown from false.

For repeatable routing and verification-plan evaluation, see [Agent Guidance Evaluation](../development/AGENT_EVALUATION.md).

<div align="right"><sub><a href="#top">🡩 Go to the top of the document</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
