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

The configurator uses a honey/yellow ANSI palette with compact Unicode symbols chosen for predictable terminal rendering. Green and red are reserved for success/failure states. Set `NO_COLOR=1` to disable ANSI color or `JELLY_NO_ICONS=true` to use ASCII fallback markers. The full header artwork is loaded from `assets/quickstart-full-logo.txt` and rendered in the honey accent color. Keep each line at 100 characters or fewer. Set `JELLY_LOGO_FILE=/path/to/logo.txt` to use a different file without editing the script.

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

The MCP surface is derived from the existing jelly registries rather than from a second hand-written tool catalog.

```mermaid
flowchart LR
    C[MCP client] --> H[/mcp]
    H --> A[OAuth / bearer validation]
    A --> M[MCP adapter]
    M --> B[Browser primitive registry]
    M --> S[System tool registry]
    B --> P[BrowserSession / Chromium]
    S --> X[System tool execution]
```

Browser primitives get named MCP arguments from their existing `ArgSpec` definitions. System tools keep their existing registry metadata and have a small adapter that maps structured MCP arguments onto their CLI forms.

The current system-tool mappings are:

```text
open-browser
close-browser
browser-task
profile-import
screenshot
downloads
wait-download
verify-artifact
inspect-network
call-routine
hitl
```

`hitl` is transport-agnostic at the MCP/routine surface. Telegram is its current implementation.

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

The HTTP MCP layer is stateless. Browser continuity comes from jelly's persistent Chromium process and runtime state.

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
