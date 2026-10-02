# Security

Jelly controls a real browser and can expose that control through MCP. Treat authentication, OAuth, tunnel, and browser-control bugs as security-sensitive.

## Reporting

Do not open a public issue for a suspected vulnerability.

**Security contact:** [jelly@fabioflorey.com](mailto:jelly@fabioflorey.com?subject=Jelly%20security%20report&body=Hi%2C%0A%0AI%27d%20like%20to%20report%20a%20security%20issue%20in%20Jelly.%0A%0AAffected%20version%20or%20commit%3A%20%0AImpact%3A%20%0AReproduction%20steps%3A%20%0AAdditional%20details%3A%20%0A%0AThanks.).

Report suspected vulnerabilities privately to that address or to the repository owner through GitHub. Include the affected version/commit, reproduction steps, impact, and any relevant logs with secrets removed.

## Browser-control authority

Jelly exposes several browser-control layers with different authority. They are not interchangeable security boundaries.

- **Semantic Jelly operations** are named capabilities with validated arguments, logical-target/ref handling, typed failures, and Jelly-specific workflow semantics. Verification remains explicit: successful execution is not automatically proof of resulting state.
- **Page JavaScript** (`evaluate-js` / `inject-js`) executes code in one page target. It can inspect or mutate state available to that page execution context, but it is not equivalent to browser-level DevTools authority.
- **Raw target-scoped CDP** sends a DevTools command directly to an attached page session. Depending on the protocol domain, it can reach runtime, DOM, input, network, storage, page, debugging, and related instrumentation outside Jelly's semantic-operation policy.
- **Raw browser-scoped CDP** sends directly to Chromium's browser connection and has the broadest exposed browser authority. Depending on the protocol domain, it can affect targets, browser contexts, permissions, persisted browser state, downloads, networking, and other browser-wide state unavailable to ordinary page JavaScript.

Raw CDP is disabled by default and is published only when the operator explicitly enables `[mcp].raw_cdp`: small-surface exposes it through `browser-call`, while large-surface exposes the dedicated raw-only `cdp-call` tool. MCP authentication still applies; enabling raw CDP does not create an unauthenticated endpoint. Jelly intentionally does not maintain a speculative per-method allowlist yet: when enabled, syntactically valid target- or browser-scoped CDP commands are treated as privileged escape-hatch operations. Enable it only for clients and workflows trusted with that additional authority.

The current authorization boundary is deployment-wide, not per-client: all published MCP tools use the same OAuth `jelly` scope. If raw CDP is enabled for the MCP process, every authenticated client authorized for that scope can use the published raw method forms. The default persistent MCP `BrowserSession`, including retained browser-event state and subscription IDs, is likewise process-wide rather than isolated per authenticated client. Changing `[mcp].raw_cdp` requires restarting the MCP process because the validated Agent Tool Catalog and raw-CDP schema are frozen at process startup. A future multi-tenant deployment should introduce per-client session/subscription isolation plus a separate capability/scope or equivalent authorization boundary rather than relying on the global process model.

Protocol success from raw CDP does not imply Jelly semantic guarantees. Raw calls can bypass target/ref abstractions, interaction conventions, verification steps, and future browser-policy enforcement. They do not replace HITL where human intervention is required by the workflow.

## Secrets

Never include `.env`, bearer tokens, OAuth secrets, tunnel tokens, Telegram credentials, cookies, or browser profile data in reports.
