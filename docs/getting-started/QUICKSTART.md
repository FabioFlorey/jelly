<div id="top"></div>

# Getting Started

**Audience:** Developers and operators setting up Jelly on a Linux workstation. This tutorial starts the MCP server, checks its HTTP endpoints, and demonstrates a browser operation.

## Prerequisites

Install the software listed in [Requirements](./REQUIREMENTS.md): Git, Rust/Cargo, Chromium, Bash, common shell utilities, and a working `systemd --user` manager. For remote ChatGPT access through a temporary HTTPS tunnel, also install `cloudflared`. Jelly currently expects Chromium at `/usr/bin/chromium`.

Run commands from the repository root. Endpoint examples use the default MCP listener at `127.0.0.1:8787`; substitute your configured address if different.

## 1. Get the source

```bash
git clone https://github.com/FabioFlorey/jelly.git
cd jelly
```

If you already have a checkout, enter that directory instead of cloning it.

## 2. Check dependencies and preview setup

```bash
./scripts/dev.sh doctor
./scripts/dev.sh setup --dry-run
```

`doctor` reports missing prerequisites. `setup --dry-run` asks configuration questions and prints a redacted summary. It does not write `.env`, compile binaries, or modify services. With an existing `.env`, the dry run loads saved settings only if a maintenance binary has already been built; otherwise it starts with defaults and asks you to supply values.

## 3. Choose the hosting mode

The checked-in `config/jelly.toml` currently defines two onboarding profiles: **ChatGPT via remote HTTPS + OAuth**, and **generic MCP via local HTTP + a bearer token**. These are setup profiles; the server still accepts both supported credential types. The remote profile is validated at startup even when no ChatGPT client is connected.

- **ChatGPT / remote access:** Choose `quick-tunnel`, `nip-io`, or `cloudflare-fixed` in the wizard. The selected hosting wrapper must establish a valid HTTPS `JELLY_PUBLIC_URL` before starting the MCP process. Quick Tunnel creates a temporary URL that changes when restarted, requiring ChatGPT reconnection.
- **Local-only generic MCP:** Before starting Jelly, remove the `[[mcp.connections]]` block with `provider = "chatgpt"` from `config/jelly.toml` (or remove all explicit connection profiles). Leave the `generic-mcp` profile in place if desired, and select `local` hosting. This avoids rejecting a local-only startup for lack of an HTTPS public URL. No unauthenticated or stdio MCP listener is provided.

For profile rules and security implications, see [Configuration](./CONFIGURATION.md) and [MCP Authentication](../reference/MCP.md#authentication).

## 4. Install and start Jelly

```bash
./scripts/dev.sh setup
```

Follow the prompts for hosting, OAuth consent and local secrets. The wizard can install/start user services, rebuilding release binaries as required. Choose service installation/startup to complete this tutorial.

The wizard writes `.env` locally; do not commit it or paste its secret values into logs or bug reports.

## 5. Verify the service

```bash
./scripts/dev.sh status
curl --fail --silent --show-error http://127.0.0.1:8787/health
curl --fail --silent --show-error http://127.0.0.1:8787/ready
```

The service status should show the managed MCP service running. `/health` returns JSON containing `"status":"ok"`, and `/ready` returns `"ready":true` with `"component":"mcp-server"`. **This does not assert that Chromium is already running.**

To verify the authenticated MCP tool list, load the local secret in a shell (without echoing it), then request `tools/list`:

```bash
# Load environment values without evaluating the .env file as shell code.
source ./scripts/lib/config.sh
jelly_load_env
curl --fail --silent --show-error http://127.0.0.1:8787/mcp \
  -H "Authorization: Bearer $JELLY_MCP_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

Expect a JSON-RPC result containing a `tools` array. The current small MCP surface publishes `browser-schema`, `browser-call`, and `browser-events` alongside system tools. Treat any HTTP `401` as an authentication failure rather than proof that the service is unhealthy.

## 6. Perform a browser operation

**The following commands use Jelly’s shared browser.** Opening a URL changes its active page and may affect other clients using the same browser. Run them on a dedicated Jelly installation or when no other workflow is active.

```bash
cargo run --quiet --bin agent-run -- open-browser https://example.com
cargo run --quiet --bin agent-run -- read-page
```

The first command opens or attaches the browser at the example page; the second reads its content. `close-browser` stops the shared browser, so do not use it as routine cleanup while other clients may be connected. For tests that do not use the installed browser, run `./scripts/dev.sh test --isolated`.

## Troubleshooting

- **Service exits with “remote-http requires a public HTTPS URL” or “invalid remote MCP public URL”:** The active remote profile has no HTTPS issuer. Use a supported remote hosting wrapper, or remove the remote profile for local-only hosting as described in step 3.
- **`/health` is unreachable:** Check `systemctl --user status jelly-mcp.service` and `journalctl --user -u jelly-mcp.service -n 50 --no-pager`. Verify the configured listener address and that no other process uses its port.
- **MCP returns `401 Unauthorized`:** Verify `JELLY_MCP_TOKEN` is loaded in the calling shell, or complete the configured OAuth flow.
- **Browser commands fail:** Verify Chromium exists at `/usr/bin/chromium` and the user service manager works. A healthy MCP server does not mean the browser is running.
- **Quick Tunnel URL changes:** Reconnect the ChatGPT MCP client; an authorization bound to the old public origin cannot be reused.

For connection and OAuth details, see [MCP Server](../reference/MCP.md). Review [Runtime Layout](../reference/RUNTIME.md#destructive-cleanup) before removing runtime data.

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
