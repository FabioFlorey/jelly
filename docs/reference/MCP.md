<div id="top"></div>

# MCP Server

Jelly exposes browser primitives and system tools through a Streamable HTTP Model Context Protocol (MCP) endpoint. The default loopback address is:

```text
http://127.0.0.1:8787/mcp
```

Service, operational, and OAuth endpoints are also exposed:

```text
/
/index
/dashboard
/connections
/health
/ready
/status
/status.json
/admin/cleanup-inactive
/.well-known/oauth-protected-resource
/.well-known/oauth-authorization-server
/register
/authorize
/pair
/pair/code
/pair/status
/token
```

`/` is the Jelly home page (`/index` remains a compatibility alias), and `/connections` lists configured MCP provider profiles without exposing credentials or claiming an authenticated session. All HTML pages share the Jelly logo, fonts, honey palette and `assets/jelly.css`. Unknown routes use the same branded 404 shell.

`/health` and `/ready` are **public, minimal service probes**. `/ready` reports readiness to accept MCP requests, not browser availability. Browser readiness and runtime diagnostics are separate, private information. Detailed `/dashboard` and `/status.json` now require a paired owner session or a bootstrap/static bearer token; `/status` is a backward-compatible JSON alias with the **same authorization requirement**. OAuth access tokens alone do not grant administration access. The dashboard polls `/status.json` every two seconds and reports runtime paths, process metrics, browser/recording state and OAuth counts only to authorized owners. If using paired consent, visit `/pair` to authorize the dashboard; in browser-password consent mode, use an authorized local API client for detailed JSON diagnostics. Existing scripts make authenticated status requests.

`POST /admin/cleanup-inactive` is the destructive operational exception and requires a valid paired-owner session cookie, so it is actionable only when `JELLY_OAUTH_CONSENT_MODE=paired`. On Linux it revalidates candidate PIDs against the running Jelly MCP executable and sends `SIGTERM` only to same-executable Jelly MCP processes that have no listening socket; non-Linux process discovery yields no cleanup candidates.

## Setup

For installation, first-run configuration, verification, and troubleshooting, follow
[Getting Started](../getting-started/QUICKSTART.md).

```bash
./scripts/dev.sh doctor
./scripts/dev.sh setup
```

Configuration and CLI branding are described in [Configuration](../getting-started/CONFIGURATION.md)
and [Development](../development/DEVELOPMENT.md). The following sections document
manual startup, MCP transport, authentication, and hosting behavior.

### Manual startup and profile validation

Jelly's `config/jelly.toml` currently includes a **remote HTTPS ChatGPT profile** and a **local HTTP generic-MCP profile**. At startup, Jelly validates **all** explicit profiles before binding. The remote profile requires an HTTPS public URL even when the operator intends to use only a local client.

For **local-only manual operation**, remove the `[[mcp.connections]]` block with `provider = "chatgpt"` in `config/jelly.toml`, or remove all connection profiles. Retaining only the `generic-mcp` profile does not require a public URL. The following example uses OpenSSL to generate credentials. From the repository root, configure secrets and start the process:

```bash
export JELLY_MCP_TOKEN="$(openssl rand -hex 32)"
export JELLY_BOOTSTRAP_SECRET="$(openssl rand -hex 32)"
export JELLY_OAUTH_CONSENT_MODE=browser
export JELLY_OAUTH_PASSWORD="$(openssl rand -hex 24)"
export JELLY_MCP_ADDR=127.0.0.1:8787
cargo run --bin jelly-mcp
```

**Expected result:** The process reports its listener at `http://127.0.0.1:8787/mcp` and continues running. In a second terminal, `curl --fail http://127.0.0.1:8787/health` should return JSON with `"status":"ok"`. An already configured `.env` is also loaded, but these exported variables take precedence.

For **remote ChatGPT operation**, leave the remote profile enabled. Use the [hosting wrapper](#hosting-modes) or the interactive wizard to establish the TLS endpoint and set `JELLY_PUBLIC_URL` to its **real HTTPS origin** before the server starts. Setting an arbitrary URL satisfies validation but does not configure a working reverse proxy. In fixed-domain mode, the proxy must forward the public URL to the local listener. The public origin is the OAuth issuer; the protected resource is `<origin>/mcp`.

The default listener is `127.0.0.1:8787`; override it with `JELLY_MCP_ADDR` if needed. `.env` is gitignored, and `.env.example` contains placeholder values rather than credentials.

## Tool mapping

The MCP adapter publishes a validated **Agent Tool Catalog**. The catalog is a projection layer: bindings still point to Jelly's canonical browser-primitive/system-tool definitions or to native Agent API builtins, but publication and execution use the same catalog allowlist rather than iterating internal registries directly. Remote clients discover this published surface through `tools/list`; the generated `.agent/tools/index.md` and `agent-discover` describe internal capabilities instead and are not substitutes for MCP discovery.

The MCP surface is selected by `config/jelly.toml` at `[mcp].surface`. Accepted values are `large-surface` and `small-surface`; unknown values are configuration errors and MCP startup fails.

`large-surface` publishes the existing individual browser primitives plus all current system tools. `small-surface` publishes `browser-schema`, `browser-call`, and `browser-events` instead of individual browser primitives, while preserving the same system-tool set until the separate system-tool compaction step. Ordinary small-surface mode never publishes both browser surfaces at once. The selected catalog is used for both `tools/list` and `tools/call`, so an internal primitive omitted from small-surface mode cannot be executed by guessing its individually published name. Measured large/small surface schema footprint and local runtime results are recorded in [`MCP_SURFACE_BENCHMARKS.md`](../development/MCP_SURFACE_BENCHMARKS.md).

The following is the **recommended least-privilege setting** for deployments that do not need direct Chromium commands; it is **not** the checked-in value:

```toml
[mcp]
surface = "small-surface"
raw_cdp = false
```

The current repository configuration sets `raw_cdp = true`. All authenticated MCP clients can therefore access the privileged raw-CDP capability unless the operator disables it and restarts Jelly.

`[mcp].raw_cdp` applies to both browser surfaces. In large-surface mode, enabling it publishes a dedicated raw-only `cdp-call` tool alongside the individual semantic primitives. In small-surface mode, enabling it adds raw CDP method forms to `browser-call`. Invalid values fail startup in either mode.
The setting is process-wide and takes effect at MCP startup: the active Agent Tool Catalog is frozen for the process lifetime, so changing `[mcp].raw_cdp` requires an MCP restart. Jelly currently publishes one OAuth scope, `jelly`; therefore raw-CDP enablement is not per-client; every authenticated client with that scope sees the same raw capability when it is enabled.

### Raw CDP trust boundary

Raw CDP is a privileged escape hatch, not another spelling of a Jelly semantic operation. With raw CDP disabled, small-surface `browser-call` publishes only `{call:{jelly,...}}` entries and large-surface publishes no raw-CDP tool. With raw CDP enabled, small-surface `browser-call` additionally exposes explicit `scope:"target"` and `scope:"browser"` method forms, while large-surface publishes the raw-only `cdp-call` tool with the same scoped method forms.

The authority levels are intentionally distinct:

```text
Jelly semantic operation
  validated named capability + Jelly target/ref/error semantics

page JavaScript (evaluate-js / inject-js)
  code in one page execution context

raw CDP scope=target
  direct privileged DevTools command to one attached page session

raw CDP scope=browser
  direct privileged DevTools command to Chromium's browser connection
```

Depending on the protocol method, target-scoped raw calls can access runtime, DOM, input, network, storage, page, debugging, and related capabilities beyond Jelly's semantic API. Browser-scoped calls can additionally affect browser-wide targets, contexts, permissions, persisted browser state, downloads, networking, and other Chromium state. Raw calls therefore can bypass Jelly-level interaction conventions, verification abstractions, ref handling, and future policy guards. MCP authentication still applies, and raw CDP remains operator-controlled by `[mcp].raw_cdp`.

Jelly currently validates raw call structure, explicit scope, logical-target routing, parameter object shape, and `Domain.command` syntax. It deliberately does **not** maintain a broad speculative CDP method allowlist in this rollout step. Enabling raw CDP therefore means trusting the authenticated client with the Chromium protocol authority exposed by the selected target/browser scope. See [`SECURITY.md`](../../SECURITY.md) for the security boundary.

```mermaid
flowchart LR
    C[MCP client] --> H[/mcp]
    H --> A[OAuth / bearer validation]
    A --> M[MCP adapter]
    M --> G[Validated Agent Tool Catalog]
    G --> B[Browser primitive bindings]
    G --> S[System tool bindings]
    G --> N[Native Agent API builtins]
    B --> P[BrowserSession / Chromium]
    N --> P
    S --> X[System tool execution]
```

Browser primitive bindings get named MCP arguments from their existing `ArgSpec` definitions through the canonical named-argument adapter. System tools keep their registry metadata and map structured MCP arguments onto their CLI forms. A capability omitted from the selected Agent Tool Catalog is neither advertised nor executable by guessing its internal name.

System-tool execution normally requires the matching `agent-<tool>` binary beside the running `jelly-mcp` executable. Jelly does not silently fall back to compiling/running the checkout, because that could make a release server execute source-tree code different from the installed build. For explicit development workflows only, `[mcp].allow_cargo_fallback = true` enables the old `cargo run` fallback.

The current system-tool mappings are intentionally identical in both large-surface and small-surface browser modes:

```text
open-browser
close-browser
browser-task
profile-import
screenshot
record-browser
download
downloads
wait-download
verify-artifact
inspect-network
call-routine
hitl
```

The large-surface browser mode is retained for at least one tagged release after the small-surface-default cutover. Removing it requires all deterministic rollback/Agent API coverage to remain green, no known supported integration to require individually published browser primitive names, and a changelog/docs notice that treats removal as a deliberate compatibility break. This is a rollout boundary, not a claim that the current system-tool surface is final.

Future review separately considers: lifecycle tools (`open-browser`, `close-browser`, `browser-task`) may become one lifecycle facade; artifact tools (`screenshot`, `record-browser`, `download`, `downloads`, `wait-download`, `verify-artifact`) may become one artifact facade; `call-routine` may become a routine facade. `inspect-network` remains separate pending stronger evidence for aggregation. `hitl` remains explicit because human intervention is a distinct workflow boundary. `profile-import` is treated as operator/admin capability and should receive a separate exposure review rather than being mechanically folded into a general lifecycle facade.

### Browser operations and artifacts

`screenshot` captures browser-rendered content and can target a specific page element without scrolling the live page as a side effect. `highlight` draws a subtle Jelly-honey, pointer-transparent overlay around a visible target; `clear-highlight` removes it. Because the overlay is rendered in the page, it appears naturally in viewport screenshots and browser recordings without modifying the target element itself. `record-browser` supports `continuous` mode for renderer-frame video and `steps` mode for an action trace video built from browser screenshots held for a configurable duration. Both use FFmpeg and neither records the desktop. `download` is the first-class browser download lifecycle tool: `list`/`status` expose Chromium GUIDs, progress, suggested filenames and terminal state; `cancel` requests `Browser.cancelDownload`; `wait` waits for a terminal result, registers the managed download as an artifact, and can copy it into an explicit destination directory under `fail`, `overwrite`, or race-safe `uniquify` collision policy. The managed GUID-named original is retained.

`downloads` remains the legacy filesystem listing, and `wait-download` remains compatible with timestamp/name workflows while preferring matching lifecycle state before falling back to the filesystem heuristic. `hitl` is transport-agnostic at the MCP/routine surface. Telegram is its current implementation and can attach either the browser viewport or a requested page target.

### Cookies and page storage

Cookies and DOM storage are semantic browser primitives rather than system tools. `cookies`, `set-cookie`, `delete-cookie`, and `clear-cookies` operate through Chromium's cookie API and can include HttpOnly cookies; an omitted cookie URL uses the active page URL. `storage-list`, `storage-get`, `storage-set`, `storage-remove`, and `storage-clear` address only the active page origin's `localStorage` or `sessionStorage`; opaque origins are rejected. In the default small surface these operations are discovered with `browser-schema` and executed through `browser-call`; large surface publishes them individually. Cookie and storage values are sensitive browser-session data.

## Tool result contract

Every `tools/call` response uses an object envelope in `structuredContent`:

```json
{"ok":true,"data":{},"error":null,"meta":{"tool":"read-page"}}
```

Tool failures keep the same shape and set `isError: true`:

```json
{"ok":false,"data":null,"error":{"kind":"target_stale","message":"...","retryable":true},"meta":{"tool":"click"}}
```

The inner `data` value may be an object, array, scalar, or null; clients never receive a raw top-level array/string as `structuredContent`. Verification and graph semantics are documented in [Reliability](../guides/RELIABILITY.md).

All MCP tools declare an OAuth security scheme with the `jelly` scope.

## Protocol surface

The current MCP implementation supports:

```text
initialize
ping
tools/list
tools/call
notifications such as notifications/initialized
```

The HTTP MCP transport is stateless. Browser continuity comes from Jelly's persistent Chromium process and shared runtime state, while the MCP server also caches one process-wide `BrowserSession` for browser-bound catalog entries by default to avoid repeated CDP connect/attach work. That session, including its retained event ring and `browser-events` subscriptions, is shared across authenticated MCP clients in the current single-owner deployment model; it is not isolated per OAuth client.

The session maintains page-only flattened auto-attach mappings internally, so Agent API calls and `browser-events` use logical targets (`main`, `tab-N`) rather than CDP `sessionId` values. Set `[mcp].persistent_session = false` to disable that cache for rollback diagnostics. Runtime-scoped `browser-events` subscriptions require the cached persistent BrowserSession; when persistent sessions are disabled, that builtin returns `unsupported` rather than creating an ID that could not survive to the next poll.

## Authentication

Jelly separates OAuth policy into the same three concerns used by PiLink:

```text
JELLY_OAUTH_CONSENT_MODE=browser|paired
JELLY_OAUTH_PUBLIC_CHATGPT_DCR=true|false
JELLY_BOOTSTRAP_SECRET=<secret>
```

`browser` consent renders an authorization page and requires `JELLY_OAUTH_PASSWORD` for each approval.

`paired` consent supports interactive local approval and a pairing fallback.

### ChatGPT local approval

When paired consent and public ChatGPT dynamic client registration (DCR) are enabled, the installer calls `POST /admin/oauth/setup/open` **with the bootstrap bearer secret** on the loopback-only admin listener and receives a five-minute `/connect?setup=...` capability. The installer opens that link in the browser. A plain `GET /connect` only displays information: it **never starts or renews setup**. The bootstrap secret is not sent to or displayed by the browser. The local `/connect` and `/approve` pages use the same branded assets and restrictive content-security policy. `/connect` displays the MCP URL.

When `JELLY_CHATGPT_PLUGIN_URL` is configured, its main action opens that private Jelly plugin directly in ChatGPT; otherwise it falls back to copying the MCP URL and opening the generic ChatGPT custom-MCP setup, where OAuth should be selected instead of a bearer token.

After ChatGPT starts the DCR and Proof Key for Code Exchange (PKCE) OAuth flow, Jelly recognizes that exact authorization while the setup session is active and redirects the browser from the public `/authorize` endpoint to loopback `/approve?setup=...&id=...`. The approval page shows the client, callback, and requested access with explicit **Yes, allow** and **No, deny** controls. The setup token and approval ID are unguessable, short-lived capabilities tied to the pending request. Submitting either choice consumes the pending request and completes the authorization decision, returning the browser to ChatGPT's registered callback in the same flow.

The local connect/approval UI is served only by a second loopback listener (`JELLY_MCP_ADMIN_ADDR`, default `127.0.0.1:8788`) that is started only for `paired` + public ChatGPT DCR; non-loopback binds are rejected and these routes are not mounted on the public MCP/OAuth listener.

### Pairing fallback

`JELLY_BOOTSTRAP_SECRET` remains required for administrative setup APIs and the headless/browser-pairing fallback, but not for the normal local `/connect` browser path. Headless, redirected, non-TTY, or unsuccessful browser setup falls back to owner-browser pairing: the bootstrap secret mints a one-time `/pair` link that expires after five minutes and is consumed once. The manual `/pair` form remains available as a fallback. `JELLY_OAUTH_PASSWORD` is not required in paired mode.

### Client registration and bearer tokens

Dynamic Client Registration is policy-gated. When `JELLY_OAUTH_PUBLIC_CHATGPT_DCR=true`, unauthenticated registration is accepted only for ChatGPT's HTTPS callback on `chatgpt.com`, including the current `/connector_platform_oauth_redirect` path and the legacy `/connector/oauth/...` form. Other registrations require `Authorization: Bearer <JELLY_BOOTSTRAP_SECRET>`. When public ChatGPT DCR is disabled, the authorization-server metadata omits the registration endpoint.

The MCP endpoint accepts two bearer-token paths:

1. `JELLY_MCP_TOKEN` for local/manual clients.
2. OAuth access tokens issued by Jelly's authorization server.

Missing or invalid credentials receive HTTP `401 Unauthorized` with a `WWW-Authenticate` challenge pointing to the protected-resource metadata endpoint.

The OAuth implementation supports authorization code + PKCE S256, rotating refresh tokens, DCR, RFC 9207 `iss`, resource binding, scope validation, and opaque access tokens. Authorization codes are single-use and expire after five minutes. Access tokens expire after 24 hours. Refresh tokens expire after 30 days, are stored only by SHA-256 fingerprint, rotate on every successful use, and retain consumed-token fingerprints long enough to detect replay; replay revokes the active refresh token and access tokens in that token family.

Registered OAuth clients, unexpired access tokens, and hashed refresh-token state persist under:

```text
/data/jelly-runtime/state/oauth.json
```

The file is mode `0600`. Owner pairing sessions are intentionally memory-only and disappear on MCP server restart, but an already-authorized client with a live refresh token can rotate it after restart without re-establishing the owner session.

## Hosting modes

Jelly supports three public hosting paths plus local-only operation:

```text
JELLY_HOSTING_MODE=local
JELLY_HOSTING_MODE=quick-tunnel
JELLY_HOSTING_MODE=nip-io
JELLY_HOSTING_MODE=cloudflare-fixed
```

`local` starts only the loopback MCP server.

`quick-tunnel` behaves like PiLink's temporary hosting path. Jelly starts `cloudflared tunnel --url ...`, waits for the generated `https://*.trycloudflare.com` URL, sets that URL as the OAuth issuer/resource, and only then starts the MCP server. No Cloudflare account or tunnel token is required. The URL changes when the tunnel is recreated, so ChatGPT must reconnect and OAuth resource-bound tokens from the old URL no longer apply.

`nip-io` exposes the machine directly over HTTPS using Caddy and a hostname derived from the public IPv4 address, for example `https://jelly-203-0-113-10.nip.io`. Caddy listens on local TCP 8080/8443 and reverse-proxies the loopback MCP server. The router must expose public TCP 80 to local 8080 and public TCP 443 to local 8443.

`JELLY_NIP_IO_NETWORK=manual` assumes those forwards already exist. `JELLY_NIP_IO_NETWORK=auto` attempts temporary mappings through `upnpc` after inspecting existing router mappings; it refuses to overwrite known mappings. NAT-PMP automatic mapping is disabled because mapping ownership cannot be verified safely; use manual forwarding when only `natpmpc` is available. If inspection or mapping fails, the launcher stops with manual-forwarding instructions. Direct nip.io hosting requires a reachable public IPv4 address and will not work behind CGNAT. `JELLY_PUBLIC_IPV4` and `JELLY_NIP_IO_HOSTNAME` can override discovery/derivation. Caddy is resolved from `JELLY_CADDY_BIN`, `caddy` on PATH, or PiLink's private Caddy binary if present.

`cloudflare-fixed` is the persistent path. Configure:

```text
JELLY_PUBLIC_URL=https://jelly.example.com
JELLY_CLOUDFLARE_TUNNEL_TOKEN=<named tunnel token>
```

The stable hostname must proxy to `http://127.0.0.1:8787`. The MCP and fixed-tunnel services restart independently while preserving the same OAuth issuer/resource URL.

The launcher resolves `cloudflared` in this order:

```text
JELLY_CLOUDFLARED_BIN
cloudflared from PATH
~/.config/pilink/bin/cloudflared
```

The last fallback lets Jelly reuse PiLink's private `cloudflared` binary without depending on PiLink's tunnel configuration.

Install the user services with:

```bash
./scripts/dev.sh install
```

For `quick-tunnel`, the MCP service owns both the temporary tunnel and MCP child process. For `nip-io`, the MCP service owns the MCP and Caddy child processes and any temporary router mappings. For `cloudflare-fixed`, `jelly-mcp.service` runs the MCP server and `jelly-cloudflared.service` runs the named tunnel.

Inspect them with:

```bash
./scripts/dev.sh status
```

**Warning:** `./scripts/dev.sh clean --yes` stops the services and deletes OAuth tokens, browser profiles and cookies, artifacts, custom extensions/userscripts **and the Cargo build directory**, even without `--build`. Read the complete [destructive cleanup contract](./RUNTIME.md#destructive-cleanup) before using it.

## Local bearer smoke test

**Prerequisite:** A running MCP server, plus an exported `JELLY_MCP_TOKEN` that matches the server configuration. From another terminal:

```bash
curl -X POST http://127.0.0.1:8787/mcp \
  -H "Authorization: Bearer $JELLY_MCP_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

A successful response has a JSON-RPC `result` containing a `tools` array. An HTTP `401` indicates that the supplied bearer token was rejected.

OAuth metadata:

```bash
curl http://127.0.0.1:8787/.well-known/oauth-protected-resource
curl http://127.0.0.1:8787/.well-known/oauth-authorization-server
```

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
