# Jelly MCP operating instructions

Jelly is a browser instrumentation layer for a persistent Chromium session.

## Rules

- Inspect before mutation unless the required state is already established by current evidence.
- Use snapshot-interactive for actionable elements. Prefer returned @eN references. After navigation, rerender, or target_stale, inspect again. Do not guess stale references.
- MCP: construct calls from the advertised inputSchema. Use named fields with the declared types. Do not translate MCP calls into positional CLI arguments.
- CLI: canonical form is `agent-run <tool> [args...]`. The tool usage string defines argument order and optional arguments.
- Actions prove only that Jelly dispatched an action. They do not prove the application reached the intended state.
- If a later operation depends on a fact, establish that fact first with wait-for or assert-*.
- Use bounded waits. Do not use arbitrary sleeps when an observable condition exists.
- Do not automatically retry side-effecting operations. Re-inspect state, classify the failure, then decide whether a retry is safe.
- Use typed error.kind values for recovery. Do not infer recovery policy by parsing error prose.
- Register important screenshots and downloads as artifacts. Use artifact verification metadata when downstream work depends on provenance.
- Before HITL, exhaust legitimate automatable paths: re-inspect, retry non-side-effecting observations, use alternate documented UI paths, refresh stale targets, navigate through supported site flows, and use equivalent controls when available.
- Do not defeat or circumvent CAPTCHA, MFA, authentication challenges, rate limits, access controls, or explicit human-verification/security mechanisms.

## HITL

- Attempt legitimate automation first unless the remaining step itself is a security or identity-verification mechanism.
- If the host has an active ChatGPT conversation, request the required human action or decision directly in that chat.
- Otherwise use Jelly's hitl tool, which delivers the request through Telegram.
- Escalate when the task reaches CAPTCHA, MFA, authentication challenges, explicit human verification, or another security control that cannot be completed through the site's supported flow without human action.
- Also escalate when legitimate browser automation cannot establish the required state with current evidence.
- After HITL, re-inspect the browser and verify the required state before continuing.

## Tool documentation

- MCP tool definitions and inputSchema returned by tools/list are authoritative for remote calls.
- Generated tool index: .agent/tools/index.md
- Tool discovery and schema loading: docs/DISCOVERY.md
- MCP behavior and remote tool mapping: docs/MCP.md
- Browser/system architecture: docs/ARCHITECTURE.md
- Guarded routines and continuation: docs/ROUTINES.md
- Verification, artifacts, errors, timeouts, and cleanup: docs/RELIABILITY.md
- Runtime paths and state: docs/RUNTIME.md
- CLI discovery: `agent-discover capabilities`, `agent-discover search <query>`, and `agent-discover schema <tool>`.

## Cookie consent

- Reject nonessential cookies by default.
- Prefer Reject, Reject all, Decline, Necessary only, or the most restrictive equivalent.
- Do not select Accept all or enable optional analytics, advertising, personalization, or tracking cookies unless the user explicitly requests it.
- If the site blocks required work unless nonessential cookies are accepted, request HITL through ChatGPT chat or Telegram before accepting them.

## Tool selection

- Use direct primitives for short, local interactions where state and recovery are simple.
- Use call-routine when the workflow needs branching, loops, typed-error recovery, HITL suspension/resume, ownership cleanup, or reusable sequencing.
- Use read-page for document-level text, title, URL, and headings.
- Use snapshot-interactive before clicking, typing, selecting, dragging, or otherwise targeting interactive elements.
- Use specialized inspect-* tools when the task depends on structured inputs, links, images, elements, accessibility, tabs, or network state.
- Use wait-for when a condition may become true asynchronously and execution should block until a bounded deadline.
- Use assert-* when the condition should already be true and failure should stop or route immediately.
- Use screenshot for ad-hoc visual capture; use verified-screenshot when downstream work depends on explicit URL/target/image readiness and registered provenance.
- Use downloads for observation of existing files; use wait-download / verified-download when the workflow depends on completion and registered provenance.
- Prefer a reusable routine once the same multi-step sequence appears more than once or requires nontrivial guards.

## Execution discipline

- Inspect -> act -> verify when correctness depends on the result.
- Use the smallest tool that establishes the required observation or predicate.
- Do not claim success from transport success alone.
- Preserve machine-readable errors and evidence. Do not replace them with inferred status.
