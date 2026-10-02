<div id="top"></div>

# MCP Server

jelly exposes its browser primitives and system tools through a Streamable HTTP MCP endpoint:

```text
http://127.0.0.1:8787/mcp
```

Health and OAuth discovery endpoints are also exposed:

```text
/health
/.well-known/oauth-protected-resource
/.well-known/oauth-authorization-server
/register
/authorize
/token
```

## Quickstart

The interactive configurator is the easiest setup path:

```bash
./quickstart.sh
```

Preview the full wizard without changing the machine:

```bash
./quickstart.sh --dry-run
```

Check only the core prerequisites:

```bash
./quickstart.sh --check
```

Dry-run mode performs the dependency audit, asks the normal configuration questions, validates mode-specific requirements, and prints a redacted summary. It does not write `.env`, create backups, build binaries, or touch systemd. Normal mode preserves existing values when rerun, generates strong local secrets when requested, backs up an existing `.env`, writes the resulting file with mode `0600`, and can immediately build/install the user services.

The configurator uses a honey/yellow ANSI palette with compact Unicode symbols chosen for predictable terminal rendering. Green and red are reserved for success/failure states. Set `NO_COLOR=1` to disable ANSI color or `[ui].icons = false` to use ASCII fallback markers. The full header artwork is loaded from `assets/quickstart-full-logo.txt` and rendered in the honey accent color. Keep each line at 100 characters or fewer. Set `[ui].logo = "path/to/logo.txt"` to use a different file without editing the script.

For manual setup instead, create the local secrets:

```bash
export JELLY_MCP_TOKEN="$(openssl rand -hex 32)"
export JELLY_BOOTSTRAP_SECRET="$(openssl rand -hex 32)"
export JELLY_OAUTH_CONSENT_MODE=browser
export JELLY_OAUTH_PASSWORD="$(openssl rand -hex 24)"
cargo run --bin jelly-mcp
```

The server loads `.env` from the Jelly checkout directory. That file is gitignored; `.env.example` contains only placeholders.

The default bind address is:

```text
127.0.0.1:8787
```

Override it with `JELLY_MCP_ADDR`.

For fixed-domain OAuth behind a tunnel, set the stable public origin explicitly:

```bash
JELLY_PUBLIC_URL=https://jelly.example.com
```

Quick Tunnel mode discovers this value automatically. The public origin is the OAuth issuer; the protected resource is `<origin>/mcp`. It must use HTTPS except for localhost development.

## Tool mapping

The MCP adapter publishes a validated **Agent Tool Catalog**. The catalog is a projection layer: bindings still point to Jelly's canonical browser-primitive/system-tool definitions or to native Agent API builtins, but publication and execution use the same catalog allowlist rather than iterating internal registries directly. Remote clients discover this published surface through `tools/list`; the generated `.agent/tools/index.md` and `agent-discover` describe internal capabilities instead and are not substitutes for MCP discovery.

The MCP surface is selected by `config/jelly.toml` at `[mcp].surface`. Accepted values are `large-surface` and `small-surface`; unknown values are configuration errors and MCP startup fails.

`large-surface` publishes the existing individual browser primitives plus all current system tools. `small-surface` publishes `browser-schema`, `browser-call`, and `browser-events` instead of individual browser primitives, while preserving the same system-tool set until the separate system-tool compaction step. Ordinary small-surface mode never publishes both browser surfaces at once. The selected catalog is used for both `tools/list` and `tools/call`, so an internal primitive omitted from small-surface mode cannot be executed by guessing its individually published name. Measured large/small surface schema footprint and local runtime results are recorded in [`MCP_SURFACE_BENCHMARKS.md`](./MCP_SURFACE_BENCHMARKS.md).

```toml
[mcp]
# browser facade
surface = "small-surface"

# optional privileged raw CDP escape hatch
raw_cdp = false
```

`[mcp].raw_cdp` applies to both browser surfaces. In large-surface mode, enabling it publishes a dedicated raw-only `cdp-call` tool alongside the individual semantic primitives. In small-surface mode, enabling it adds raw CDP method forms to `browser-call`. Invalid values fail startup in either mode.
The setting is process-wide and takes effect at MCP startup: the active Agent Tool Catalog is frozen for the process lifetime, so changing `[mcp].raw_cdp` requires an MCP restart. Jelly currently publishes one OAuth scope, `jelly`; therefore raw-CDP enablement is not per-client—every authenticated client with that scope sees the same raw capability when it is enabled.

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

The current system-tool mappings are intentionally unchanged in both large-surface and small-surface browser modes:

```text
open-browser
close-browser
browser-task
profile-import
screenshot
record-browser
downloads
wait-download
verify-artifact
inspect-network
call-routine
hitl
```

The large-surface browser mode is retained for at least one tagged release after the small-surface-default cutover. Removal requires all deterministic rollback/Agent API coverage to remain green, no known supported integration to require individually published browser primitive names, and a changelog/docs notice that treats removal as a deliberate compatibility break. This is a rollout boundary, not a statement that the current system-tool surface is final. Future review is tracked separately: lifecycle tools (`open-browser`, `close-browser`, `browser-task`) may become one lifecycle facade; artifact tools (`screenshot`, `record-browser`, `downloads`, `wait-download`, `verify-artifact`) may become one artifact facade; `call-routine` may become a routine facade. `inspect-network` remains separate pending stronger evidence for aggregation. `hitl` remains explicit because human intervention is a distinct workflow boundary. `profile-import` is treated as operator/admin capability and should receive a separate exposure review rather than being mechanically folded into a general lifecycle facade.

`screenshot` captures browser-rendered content and can target a specific page element without scrolling the live page as a side effect. `highlight` draws a subtle Jelly-honey, pointer-transparent overlay around a visible target; `clear-highlight` removes it. Because the overlay is rendered in the page, it appears naturally in viewport screenshots and browser recordings without modifying the target element itself. `record-browser` supports `continuous` mode for renderer-frame video and `steps` mode for an action trace video built from browser screenshots held for a configurable duration. Both use FFmpeg and neither records the desktop. `hitl` is transport-agnostic at the MCP/routine surface. Telegram is its current implementation and can attach either the browser viewport or a requested page target.

## Tool result contract

Every `tools/call` response uses an object envelope in `structuredContent`:

```json
{"ok":true,"data":{},"error":null,"meta":{"tool":"read-page"}}
```

Tool failures keep the same shape and set `isError: true`:

```json
{"ok":false,"data":null,"error":{"kind":"target_stale","message":"...","retryable":true},"meta":{"tool":"click"}}
```

The inner `data` value may be an object, array, scalar, or null; clients never receive a raw top-level array/string as `structuredContent`. Verification and graph semantics are documented in [Reliability](./RELIABILITY.md).

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

The HTTP MCP transport is stateless. Browser continuity comes from jelly's persistent Chromium process and shared runtime state, while the MCP server also caches one process-wide `BrowserSession` for browser-bound catalog entries by default to avoid repeated CDP connect/attach work. That session, including its retained event ring and `browser-events` subscriptions, is shared across authenticated MCP clients in the current single-owner deployment model; it is not isolated per OAuth client. The session maintains page-only flattened auto-attach mappings internally, so Agent API calls and `browser-events` use logical targets (`main`, `tab-N`) rather than CDP `sessionId` values. Set `[mcp].persistent_session = false` to disable that cache for rollback diagnostics. Runtime-scoped `browser-events` subscriptions require the cached persistent BrowserSession; when persistent sessions are disabled, that builtin returns `unsupported` rather than creating an ID that could not survive to the next poll.

## Authentication

jelly separates OAuth policy into the same three concerns used by PiLink:

```text
JELLY_OAUTH_CONSENT_MODE=browser|paired
JELLY_OAUTH_PUBLIC_CHATGPT_DCR=true|false
JELLY_BOOTSTRAP_SECRET=<secret>
```

`browser` consent renders an authorization page and requires `JELLY_OAUTH_PASSWORD` for each approval.

`paired` consent first requires the owner to pair a browser at `/pair` using `JELLY_BOOTSTRAP_SECRET`. The resulting owner session is an HttpOnly, SameSite=Lax cookie with a 24-hour lifetime. OAuth approvals are rejected unless they come from that paired browser. `JELLY_OAUTH_PASSWORD` is not required in paired mode.

Dynamic Client Registration is policy-gated. When `JELLY_OAUTH_PUBLIC_CHATGPT_DCR=true`, unauthenticated registration is accepted only for ChatGPT's HTTPS callback on `chatgpt.com`, including the current `/connector_platform_oauth_redirect` path and the legacy `/connector/oauth/...` form. Other registrations require `Authorization: Bearer <JELLY_BOOTSTRAP_SECRET>`. When public ChatGPT DCR is disabled, the authorization-server metadata omits the registration endpoint.

The MCP endpoint accepts two bearer-token paths:

1. `JELLY_MCP_TOKEN` for local/manual clients.
2. OAuth access tokens issued by jelly's authorization server.

Missing or invalid credentials receive HTTP `401 Unauthorized` with a `WWW-Authenticate` challenge pointing to the protected-resource metadata endpoint.

The OAuth implementation supports authorization code + PKCE S256, DCR, RFC 9207 `iss`, resource binding, scope validation, and opaque access tokens. Authorization codes are single-use and expire after five minutes. Access tokens currently expire after 24 hours.

Registered OAuth clients and unexpired access tokens persist under:

```text
/data/jelly-runtime/state/oauth.json
```

The file is mode `0600`. Owner pairing sessions are intentionally memory-only and must be re-established after an MCP server restart.

## Hosting modes

jelly currently supports three public hosting paths plus local-only operation:

```text
JELLY_HOSTING_MODE=local
JELLY_HOSTING_MODE=quick-tunnel
JELLY_HOSTING_MODE=nip-io
JELLY_HOSTING_MODE=cloudflare-fixed
```

`local` starts only the loopback MCP server.

`quick-tunnel` behaves like PiLink's temporary hosting path. jelly starts `cloudflared tunnel --url ...`, waits for the generated `https://*.trycloudflare.com` URL, sets that URL as the OAuth issuer/resource, and only then starts the MCP server. No Cloudflare account or tunnel token is required. The URL changes when the tunnel is recreated, so ChatGPT must reconnect and OAuth resource-bound tokens from the old URL no longer apply.

`nip-io` exposes the machine directly over HTTPS using Caddy and a hostname derived from the public IPv4 address, for example `https://jelly-203-0-113-10.nip.io`. Caddy listens on local TCP 8080/8443 and reverse-proxies the loopback MCP server. The router must expose public TCP 80 to local 8080 and public TCP 443 to local 8443.

`JELLY_NIP_IO_NETWORK=manual` assumes those forwards already exist. `JELLY_NIP_IO_NETWORK=auto` attempts temporary mappings using `upnpc` first, then `natpmpc`; if neither helper exists or mapping fails, the launcher stops with manual-forwarding instructions. Direct nip.io hosting requires a reachable public IPv4 address and will not work behind CGNAT. `JELLY_PUBLIC_IPV4` and `JELLY_NIP_IO_HOSTNAME` can override discovery/derivation. Caddy is resolved from `JELLY_CADDY_BIN`, `caddy` on PATH, or PiLink's private Caddy binary if present.

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

The last fallback lets jelly reuse PiLink's private `cloudflared` binary without depending on PiLink's tunnel configuration.

Install the user services with:

```bash
scripts/install-mcp-services.sh
```

For `quick-tunnel`, the MCP service owns both the temporary tunnel and MCP child process. For `nip-io`, the MCP service owns the MCP and Caddy child processes and any temporary router mappings. For `cloudflare-fixed`, `jelly-mcp.service` runs the MCP server and `jelly-cloudflared.service` runs the named tunnel.

Inspect them with:

```bash
scripts/status-mcp-services.sh
```

A full `scripts/clean-runtime.sh` stops the services and deletes persisted OAuth client/token state, intentionally requiring authorization again.

## Local bearer smoke test

```bash
curl -X POST http://127.0.0.1:8787/mcp \
  -H "Authorization: Bearer $JELLY_MCP_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

OAuth metadata:

```bash
curl http://127.0.0.1:8787/.well-known/oauth-protected-resource
curl http://127.0.0.1:8787/.well-known/oauth-authorization-server
```

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
