# Jelly MCP operating instructions

Jelly is a browser instrumentation layer for a persistent Chromium session. Treat the MCP catalog returned by `tools/list` as the authoritative remote surface.

## Surface selection

- Detect the active surface from `tools/list`; do not infer it from environment variables or assume one rollout mode.
- **Small surface:** if `browser-schema`, `browser-call`, and `browser-events` are published, use the small-surface policy below. Individual browser primitives are intentionally not remote tools.
- **Large surface:** if individual browser primitives are published instead, use those advertised tools and their `inputSchema` directly. Do not invent small-surface facade tools that are not listed.
- System tools such as browser lifecycle, artifacts, routines, network inspection, and HITL remain top-level in both rollout surfaces. Use only tools actually advertised by `tools/list`.

## Compact browser policy

Use one selection rule:

1. Prefer a **semantic Jelly operation** whenever it expresses the intended observation, action, wait, or verification.
2. If the semantic operation name or argument contract is not already known from current evidence, use **browser-schema** to discover it. Do not guess operation names or parameter fields.
3. Execute semantic operations through **browser-call**.
4. Use **browser-events** only when the task depends on retained CDP notifications or event-stream state rather than an ordinary observation/action.
5. Use **raw CDP** only when it is published and a semantic operation cannot express the required capability, or the task genuinely requires low-level protocol state, diagnostics, browser-level control, or event-domain setup.

### browser-schema

- Use `capabilities` to understand the semantic capability set.
- Use `search` to find an operation by intent.
- Use `schema` to load the named JSON argument contract for one operation.
- The returned schema is authoritative. Do not infer positional CLI arguments for MCP calls.

### browser-call

- Semantic calls use the Jelly operation form inside `calls[]`; preserve the named JSON fields returned by `browser-schema`.
- Batches are ordered and fully preflighted before execution. Use batching when calls naturally belong to one ordered browser interaction, not merely to reduce tool-call count.
- A per-call logical `target` may address `main`, `tab-2`, and other published logical page targets. Do not supply Chromium `targetId` or `sessionId` values.
- Semantic operations may intentionally change the active page when explicitly targeted. Target-scoped raw CDP uses the target's internal attached session directly and does not imply an active-tab switch.
- Respect `on_error`. Do not use `continue` to hide a failure that invalidates later assumptions.
- Preserve returned structured errors and per-call outcomes; do not reduce them to prose success/failure.

### Raw CDP

- Raw CDP is a privileged escape hatch and may be absent entirely from the published `browser-call` schema.
- Never construct method-form calls when the schema does not publish them.
- Prefer semantic Jelly operations over raw CDP when both can express the task.
- Page JavaScript and raw CDP are not equivalent. Raw target CDP can bypass Jelly semantic/ref/verification policy; browser-scoped CDP has broader Chromium-level authority.
- Use raw CDP for missing semantic capability, protocol diagnostics, explicit low-level state, browser-level control, or event-domain enablement when required.
- A successful raw protocol response is not evidence that the workflow goal is satisfied. Verify resulting state explicitly.
- Raw CDP does not replace HITL and must not be used to circumvent CAPTCHA, MFA, authentication challenges, rate limits, access controls, or human-verification/security mechanisms.

### browser-events

- Use `subscribe`, `poll`, and `unsubscribe` for retained CDP notifications.
- Subscriptions are non-retroactive and scoped to the owning persistent BrowserSession.
- Filter with logical targets and exact method/method-prefix constraints when possible.
- Check `cursor_lost`, `dropped`, and `stream_resets`; event delivery is bounded and loss is explicit.
- Jelly does not implicitly enable arbitrary CDP event domains. If a required domain must be enabled and no semantic operation does so, raw CDP may be necessary when the operator has enabled it.
- Do not expect push delivery while Jelly is idle; polling drains pending notifications through the synchronous barrier model.

## Execution discipline

- Inspect before mutation unless the required state is already established by current evidence.
- For interactive work on the small-surface, discover/use the semantic inspection operation through `browser-schema` + `browser-call`; prefer returned Jelly element references when available.
- Jelly element refs are observations, not permanent IDs. After navigation, rerender, document replacement, or `target_stale`, inspect again. Do not guess stale references.
- Actions prove only that Jelly dispatched an action. They do not prove the application reached the intended state.
- If a later operation depends on a fact, establish that fact first with an appropriate semantic wait/assert/inspection operation.
- Use bounded waits. Do not use arbitrary sleeps when an observable condition exists.
- Do not automatically retry side-effecting operations. Re-inspect state, classify the failure, then decide whether retry is safe.
- Use typed `error.kind` values for recovery. Do not infer recovery policy by parsing error prose.
- Do not claim success from transport or CDP success alone.
- Preserve machine-readable errors and evidence.

## Artifacts and provenance

- Use the published system artifact tools for screenshots, recordings, downloads, waits, and verification.
- Register important screenshots and downloads as artifacts when downstream work depends on provenance.
- Prefer verified artifact flows when later steps depend on URL, target, readiness, or completion evidence.
- Visual annotations such as highlighting are semantic browser operations in small-surface mode; discover and call them through the small-surface browser facade rather than assuming a top-level primitive tool.

## Routines

- Use `call-routine` when the workflow needs reusable guarded sequencing, branching, loops, typed-error recovery, HITL suspension/resume, or ownership cleanup.
- Do not migrate a short browser sequence into a routine merely because small-surface mode batches semantic calls; routines and browser-call solve different problems.

## HITL

- Attempt legitimate automation first unless the remaining step itself is a security or identity-verification mechanism.
- Before HITL, exhaust legitimate automatable paths: re-inspect, retry only safe observations, refresh stale targets, and use supported alternate UI paths where appropriate.
- If the host has an active ChatGPT conversation, request the required human action or decision directly in that chat.
- Otherwise use the published `hitl` system tool, which currently delivers the request through Telegram.
- Escalate at CAPTCHA, MFA, authentication challenges, explicit human verification, or another security control that requires human action.
- Also escalate when legitimate browser automation cannot establish the required state with current evidence.
- After HITL, re-inspect and verify the required state before continuing.

## Cookie consent

- Reject nonessential cookies by default.
- Prefer Reject, Reject all, Decline, Necessary only, or the most restrictive equivalent.
- Do not accept optional analytics, advertising, personalization, or tracking cookies unless the user explicitly requests it.
- If required work is blocked unless nonessential cookies are accepted, request HITL before accepting them.

## MCP and CLI contracts

- MCP tool definitions and `inputSchema` returned by `tools/list` are authoritative for remote calls.
- MCP uses named JSON fields. Do not translate MCP calls into positional CLI arguments.
- CLI remains a separate surface: canonical form is `agent-run <tool> [args...]`, with each tool's usage string defining argument order.
- The small-surface MCP facade does not remove or rename internal semantic primitives used by CLI, routines, tests, or discovery.

## Reference documentation

- Generated internal tool index: `.agent/tools/index.md`
- Tool discovery and schema loading: `docs/DISCOVERY.md`
- MCP surface and rollout behavior: `docs/MCP.md`
- Browser/system architecture: `docs/ARCHITECTURE.md`
- Guarded routines and continuation: `docs/ROUTINES.md`
- Verification, artifacts, errors, timeouts, and cleanup: `docs/RELIABILITY.md`
- Runtime paths and state: `docs/RUNTIME.md`
- Security and raw-CDP authority: `SECURITY.md`
- CLI discovery: `agent-discover capabilities`, `agent-discover search <query>`, and `agent-discover schema <tool>`.
