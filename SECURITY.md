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

The **checked-in** `config/jelly.toml` currently has `[mcp].raw_cdp = true`, so raw CDP is enabled at startup unless the operator changes it and restarts Jelly. The recommended least-privilege setting for a deployment that does not require raw protocol access is `raw_cdp = false`. Raw CDP is published only when `[mcp].raw_cdp = true`: small-surface exposes it through `browser-call`, while large-surface exposes the dedicated raw-only `cdp-call` tool. MCP authentication still applies; enabling raw CDP does not create an unauthenticated endpoint. Jelly intentionally does not maintain a speculative per-method allowlist yet: when enabled, syntactically valid target- or browser-scoped CDP commands are treated as privileged escape-hatch operations. Enable it only for clients and workflows trusted with that additional authority.

The current authorization boundary is deployment-wide, not per-client: all published MCP tools use the same OAuth `jelly` scope. If raw CDP is enabled for the MCP process, every authenticated client authorized for that scope can use the published raw method forms. The default persistent MCP `BrowserSession`, including retained browser-event state and subscription IDs, is likewise process-wide rather than isolated per authenticated client. Changing `[mcp].raw_cdp` requires restarting the MCP process because the validated Agent Tool Catalog and raw-CDP schema are frozen at process startup. A future multi-tenant deployment should introduce per-client session/subscription isolation plus a separate capability/scope or equivalent authorization boundary rather than relying on the global process model.

Protocol success from raw CDP does not imply Jelly semantic guarantees. Raw calls can bypass target/ref abstractions, interaction conventions, verification steps, and future browser-policy enforcement. They do not replace HITL where human intervention is required by the workflow.

Semantic storage operations intentionally carry sensitive authority too. Authenticated clients can inspect cookies applicable to the active page or an explicitly supplied URL, including HttpOnly cookies, and can mutate cookies plus the active origin's localStorage/sessionStorage. Those values can represent authenticated browser sessions. `set-cookie` values and `storage-set` values are redacted from primitive trace arguments, but operation results are returned to the authenticated caller and must be handled as secrets. These semantic operations are narrower than raw browser-scoped CDP; they do not provide arbitrary profile/database access.

Interactive paired OAuth setup uses a separate loopback-only listener rather than adding setup routes to the public/tunneled MCP listener. A direct local navigation to `/connect` is informational and cannot mint a setup capability. The installer explicitly authenticates `POST /admin/oauth/setup/open` with the bootstrap secret, then opens the returned short-lived local capability URL. The bootstrap secret remains required for administrative setup APIs and pairing fallback and is never rendered into `/connect` or `/approve`. Browser setup uses an unguessable short-lived setup capability plus a separate one-time approval ID tied to the exact validated ChatGPT DCR/PKCE request. `/approve` consumes that request before issuing the OAuth redirect back to ChatGPT, so an approval URL cannot be replayed. Jelly refuses non-loopback `JELLY_MCP_ADMIN_ADDR` values. OAuth refresh tokens are persisted only as SHA-256 fingerprints and rotate on use; replay of a consumed refresh token revokes the active token family.

Detailed diagnostics (`/status.json`, legacy `/status`, and `/dashboard`) require a paired-owner cookie or a bootstrap/static bearer token; ordinary OAuth access tokens do not grant diagnostic administration. `/health` and `/ready` remain public and intentionally minimal. The provider-independent `/connections` interface shows configured connection methods but never tokens or authorization proofs. The browser-ready state is available through the protected status API, not the public MCP readiness probe.

## Secrets

Never include `.env`, bearer tokens, OAuth secrets, tunnel tokens, Telegram credentials, cookies, or browser profile data in reports.
