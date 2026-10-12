<div id="top"></div>

# Getting Started

## Table of Contents

- [Interactive CLI and product walkthrough](#interactive-cli-and-product-walkthrough)
- [Prerequisites](#prerequisites)
- [1. Get the source](#1-get-the-source)
- [2. Check dependencies and preview setup](#2-check-dependencies-and-preview-setup)
- [3. Choose the hosting mode](#3-choose-the-hosting-mode)
- [4. Install and start Jelly](#4-install-and-start-jelly)
- [5. Verify the service](#5-verify-the-service)
- [6. Perform a browser operation](#6-perform-a-browser-operation)
- [Troubleshooting](#troubleshooting)

**Audience:** Developers and operators setting up Jelly on a Linux workstation. This tutorial starts the MCP server, checks its HTTP endpoints, and demonstrates a browser operation.

## Interactive CLI and product walkthrough

Run **`./scripts/dev.sh`** in a terminal for Jelly's **real quickstart**. It checks prerequisites, asks where Chromium should run, configures MCP credentials and hosting, previews changes, and offers to install/start the service and verify its HTTP health/readiness endpoints. There is no separate home dashboard. Other commands perform their named real operations.

**Try the actual interaction without touching the host:**

```bash
./scripts/dev.sh demo                         # Real menu choices and editable inputs
./scripts/dev.sh demo --menu                  # Same interactive prompts with piped input
./scripts/dev.sh demo --tour                  # Pre-filled example session
./scripts/dev.sh demo --scenario browsers     # Setup, browsers, alternatives, manage, tests, recovery
./scripts/dev.sh demo --hero                  # Original full 97-column logo
```

In the demo, choose a sample computer with Docker, Podman, both, or neither; enter a hosting URL; select OAuth/bearer authentication; choose Chromium sandbox, restrictions, profile and CDP exposure; test a candidate; and accept or reject the final review. Results and browser/service state follow **your choices** within the same demo session. In an interactive terminal, each transition **clears and redraws the current screen with the full Jelly hero**; results remain visible until you press Enter. In a redirected transcript or `--tour`, screen clearing and Enter-to-continue are disabled, and selected sample inputs are echoed so the transcript remains readable. Nothing is read from the real Docker/Podman daemons or applied to Jelly's configuration. `--tour` fills in those same prompts with a preselected example.

The demo's **Tests → Browser integration** choice uses the sample MCP and browser state. If no browser is selected or the sample MCP service is stopped, it reports **NOT RUN** and identifies the missing prerequisite. You can separately explore an example PASS or an intentionally failing integration with its diagnostics; neither is evidence of a live test.

**For readers understanding the CLI processes:** Open [CLI Processes Diagrams](../reference/diagrams/DEV.SH.md) directly as Markdown. It contains five Mermaid diagrams with a table of contents, brief process definitions and chapter navigation. The diagrams are separate from the interactive demo.

Both CLI flows use the same honey-yellow headings, structured chapter spacing and confirmation language. `NO_COLOR` is respected.

## Prerequisites

Install the software listed in [Requirements](./REQUIREMENTS.md): Git, Rust/Cargo, Bash, common shell utilities, and a working `systemd --user` manager for the installed-service workflow. Native browser mode expects Chromium at `/usr/bin/chromium`; container/remote-CDP mode instead uses Docker/Podman or an already reachable CDP endpoint. For remote ChatGPT access through a temporary HTTPS tunnel, install `cloudflared`.

Run commands from the repository root. **Interactive terminal:** `./scripts/dev.sh` starts real quickstart, including confirmation before any browser changes or installation. **Automation:** `./scripts/dev.sh <command>` retains the explicit command interface; bare quickstart refuses to run without a terminal. Use `./scripts/dev.sh demo` only for a simulated walkthrough. Endpoint examples use the default MCP listener at `127.0.0.1:8787`; substitute your configured address if different.

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
./scripts/dev.sh
```

Quickstart first prompts for the real browser backend (native Chromium, container, external CDP, or previously configured). Then the four-stage MCP wizard (identity, hosting, optional integrations, review) displays a **redacted preview** and asks before writing `.env`. The final stage separately requests approval to install and start the real services, followed by MCP health/readiness checks. You can cancel or save configuration without installing. For MCP-only configuration, `./scripts/dev.sh setup` remains available.

After confirmation the wizard writes `.env` locally; do not commit it or paste its secret values into logs or bug reports.

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

**The following commands use Jelly’s shared native browser unless a remote browser is selected.** When remote CDP is configured, MCP calls can specify `browser_id` to target a specific browser UUID or easy name; see [Container Chromium](./CONTAINER_CHROMIUM.md) and [Browser Deployment](../reference/BROWSER_DEPLOYMENT.md). Opening a URL changes its active page and may affect other clients using the same browser. Run them on a dedicated Jelly installation or when no other workflow is active.

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
