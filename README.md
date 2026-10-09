<div id="top"></div>

<div align="center">

<img src="./assets/full-logo.png" alt="Jelly" width="760">

**Browser instrumentation for agents**

✨ [**Quickstart**](#3-quickstart)　•　⭐ [**Architecture**](./docs/architecture/ARCHITECTURE.md)　•　⚡ [**MCP**](./docs/reference/MCP.md)　•　💡 [**Documentation**](#8-documentation-and-support)　•　🍯 [**Tool Index**](./.agent/tools/index.md)

📒 [Visit the project website](https://fabioflorey.github.io/jelly/)

[![Stars](https://img.shields.io/github/stars/FabioFlorey/jelly?style=flat&label=stars)](https://github.com/FabioFlorey/jelly/stargazers) [![Forks](https://img.shields.io/github/forks/FabioFlorey/jelly?style=flat&label=forks)](https://github.com/FabioFlorey/jelly/forks) [![CI](https://github.com/FabioFlorey/jelly/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/FabioFlorey/jelly/actions/workflows/ci.yml?query=branch%3Amain+event%3Apush) [![Rust 1.98.1](https://img.shields.io/badge/rust-1.98.1-000000?logo=rust)](./rust-toolchain.toml) [![License: Proprietary](https://img.shields.io/badge/license-proprietary-FFC107)](./LICENSE)

</div>

## 1. What Jelly does

**Jelly** is a Rust Model Context Protocol (MCP) server that lets authenticated agents inspect and control a real Chromium browser, verify page state, and retain browser artifacts.

Jelly starts or attaches to Chromium and exposes browser capabilities through a local **Streamable HTTP** MCP endpoint, normally `http://127.0.0.1:8787/mcp`. A configured HTTPS tunnel or reverse proxy can expose that endpoint to a remote client.

- **Inspect:** Read page content, identify interactive elements, and discover tools through `browser-schema`.
- **Act:** Run ordered navigation, input, and page operations through `browser-call`.
- **Verify:** Use semantic assertions and typed failures to confirm what changed. An action's success is not proof of the intended outcome.
- **Capture:** Record screenshots, downloads, browser recordings, and network evidence with metadata.
- **Recover:** Execute guarded routines and request human intervention through Telegram.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>

## 2. Use cases

**Put your AI to work on the real web.**

From research to action, Jelly gives AI agents a real Chromium browser to navigate dynamic websites, work with interactive applications, and verify results.

<details name="use-case">
  <summary><b>Browser Automation: Let AI handle the repetitive work.</b></summary>
  <blockquote>
    <p>Automate tasks across websites and business applications.</p>
    <ul>
      <li>Fill out forms and submit requests.</li>
      <li>Update records and navigate dashboards.</li>
      <li>Complete multi-step workflows across pages.</li>
    </ul>
  </blockquote>
</details>

<details name="use-case">
  <summary><b>AI-Powered Research: Explore more than static web pages.</b></summary>
  <blockquote>
    <p>Research information on JavaScript-heavy websites, interactive dashboards, and dynamically loaded pages.</p>
    <ul>
      <li>Investigate markets and competitors.</li>
      <li>Compare products, features, and pricing shown on websites.</li>
      <li>Explore filters and gather findings from multiple sources.</li>
    </ul>
  </blockquote>
</details>

<details name="use-case">
  <summary><b>Browser Testing: Check real user journeys.</b></summary>
  <blockquote>
    <p>Test interactive websites using Chromium and inspect the results.</p>
    <ul>
      <li>Walk through navigation, forms, and interactive features.</li>
      <li>Check whether expected elements and page states appear.</li>
      <li>Capture screenshots to investigate failures.</li>
    </ul>
  </blockquote>
</details>

<details name="use-case">
  <summary><b>Data Extraction: Collect information from complex websites.</b></summary>
  <blockquote>
    <p>Gather information from pages that require browser interaction.</p>
    <ul>
      <li>Read interactive tables and paginated results.</li>
      <li>Apply filters and move between result pages.</li>
      <li>Retrieve documents and collect data for further analysis.</li>
    </ul>
  </blockquote>
</details>

<details name="use-case">
  <summary><b>Business Process Automation: Connect work across applications.</b></summary>
  <blockquote>
    <p>Carry out browser-based processes in tools that may not have a dedicated integration.</p>
    <ul>
      <li>Work with CRM systems and customer portals.</li>
      <li>Update entries in back-office applications.</li>
      <li>Navigate reporting tools and process routine requests.</li>
    </ul>
  </blockquote>
</details>

<details name="use-case">
  <summary><b>AI Agents: Give agents the ability to act, not just answer.</b></summary>
  <blockquote>
    <p>Connect an AI agent to Jelly through MCP to interact with websites and check the outcome.</p>
    <ul>
      <li>Open pages, inspect content, and interact with controls.</li>
      <li>Execute multi-step browser workflows and verify results.</li>
      <li>Request human input when a workflow needs approval or assistance.</li>
    </ul>
  </blockquote>
</details>

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>

## 3. Quickstart

**Requirements:** Linux with Rust **1.98.1**, Cargo, Chromium at `/usr/bin/chromium`, Bash, and systemd user services. Remote access through Quick Tunnel also requires `cloudflared`. See [complete prerequisites](./docs/getting-started/REQUIREMENTS.md).

Run the interactive setup:

```bash
git clone https://github.com/FabioFlorey/jelly.git
cd jelly
./scripts/dev.sh doctor
./scripts/dev.sh setup
```

Follow the wizard to configure credentials, hosting, and service startup.

Choose the connection mode in the wizard. For ChatGPT, use a public HTTPS endpoint with OAuth. For local MCP clients, use the loopback HTTP endpoint and bearer-token authentication.

See [Getting Started](./docs/getting-started/QUICKSTART.md) for the configuration steps for each mode.

Verify the server in another terminal:

```bash
./scripts/dev.sh status
curl --fail --silent --show-error http://127.0.0.1:8787/health
```

Expect JSON with `"status":"ok"` from `/health`.

To try browser operations on a dedicated Jelly installation:

```bash
cargo run --quiet --bin agent-run -- open-browser https://example.com
cargo run --quiet --bin agent-run -- read-page
cargo run --quiet --bin agent-run -- snapshot-interactive
```

These commands use the shared browser session. Opening a URL can change the page seen by other clients; `close-browser` stops that shared browser. For disposable tests, use `./scripts/dev.sh test --isolated`.

See the [step-by-step setup guide](./docs/getting-started/QUICKSTART.md) for client credentials, browser verification, and troubleshooting.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>

## 4. Connect your agent

Jelly exposes the browser through **Streamable HTTP MCP** and includes a CLI for direct local use.

| Connection | How to use Jelly |
| :--- | :--- |
| **ChatGPT** | Connect through a public HTTPS MCP endpoint using OAuth. |
| **MCP clients** | Connect to the local Streamable HTTP endpoint with a bearer token. |
| **Rust CLI** | Use `agent-run` and `agent-discover` directly from the terminal. |

See the [client setup guide](https://fabioflorey.github.io/jelly/clients.html) for URLs, credentials, and connection steps.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>

## 5. MCP tools

The default `small-surface` API publishes three browser tools and thirteen system tools. `large-surface` publishes individual browser primitives instead of the three browser tools. The running server's `tools/list` response is authoritative.

The `small-surface` browser tools are `browser-schema`, `browser-call`, and `browser-events`.

System tools in both surfaces are `open-browser`, `close-browser`, `browser-task`, `profile-import`, `screenshot`, `record-browser`, `download`, `downloads`, `verify-artifact`, `wait-download`, `inspect-network`, `call-routine`, and `hitl`.

For exact parameters, side effects, and output contracts, see the [MCP tool reference](https://fabioflorey.github.io/jelly/tools.html), [MCP server guide](./docs/reference/MCP.md), and the runtime's authoritative `tools/list` response.

Use `browser-schema` to discover the named semantic operations available through `browser-call`.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>

## 6. MCP usage example

With the server running and the local token available in the current shell, discover implemented browser capabilities **without opening Chromium**:

```bash
# Load environment values without evaluating the .env file as shell code.
source ./scripts/lib/config.sh
jelly_load_env

curl --fail --silent --show-error http://127.0.0.1:8787/mcp \
  -H "Authorization: Bearer $JELLY_MCP_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"browser-schema","arguments":{"action":"capabilities"}}}'
```

A successful response contains a JSON-RPC `result.structuredContent` envelope with `ok: true`, `meta.tool: "browser-schema"`, and `data.action: "capabilities"`. The operation count and categories depend on the current binary.

This request discovers browser capabilities before opening Chromium.

The [examples page](https://fabioflorey.github.io/jelly/examples.html) explains the next tool call and expected result.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>

## 7. Work around the click!

<div align="center">

<a href="https://fabioflorey.github.io/jelly/">
  <img src="./assets/porsche-718-spyder-rs-demo-20261008.gif" alt="Looping demo of Jelly configuring a yellow Porsche 718 Spyder RS" width="880">
</a>

<sub><strong>Every move, on the record.</strong> Jelly doesn't just automate the browser. It shows its work, capturing and captioning its own actions as they happen. <a href="https://github.com/FabioFlorey/jelly/blob/main/site/assets/images/porsche-718-spyder-rs-demo-20261008.mp4">Watch the full recording</a>.</sub>

</div>

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>

## 8. Documentation and support

Start with the [documentation index](./docs/INDEX.md), or go directly to the guide you need:

- **Get started:** [Installation](./docs/getting-started/QUICKSTART.md) · [Requirements](./docs/getting-started/REQUIREMENTS.md) · [Configuration](./docs/getting-started/CONFIGURATION.md)
- **Integrate with agents:** [MCP server](./docs/reference/MCP.md) · [Tool discovery](./docs/reference/DISCOVERY.md) · [Live tool reference](https://fabioflorey.github.io/jelly/tools.html)
- **Build reliable workflows:** [Routines](./docs/guides/ROUTINES.md) · [Reliability](./docs/guides/RELIABILITY.md) · [Architecture](./docs/architecture/ARCHITECTURE.md)
- **Develop and troubleshoot:** [Development](./docs/development/DEVELOPMENT.md) · [Tests](./tests/INDEX.md) · [Troubleshooting](https://fabioflorey.github.io/jelly/troubleshooting.html)

The [project website](https://fabioflorey.github.io/jelly/) includes setup instructions, connection examples, and browser-tool documentation.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>

## 9. Project information

Jelly is under active development. For updates, see the [changelog](./CHANGELOG.md) and [releases](https://github.com/FabioFlorey/jelly/releases).

**Licensing:** Free for individual, personal, non-commercial use, including private modifications. Repackaging, rebranding, white-labeling, resale, redistribution, and publishing modified versions are not permitted under the personal-use license. Business use, paid courses, monetized content, and other commercial activities require a separately signed agreement with negotiated license fees and revenue-sharing royalties. For commercial licensing, contact [jelly@fabioflorey.com](mailto:jelly@fabioflorey.com). See the [full license](./LICENSE).

[Contributing](./CONTRIBUTING.md) · [Issue tracker](https://github.com/FabioFlorey/jelly/issues) · [Security policy](./SECURITY.md) · [License](./LICENSE)

**Project contact:** [jelly@fabioflorey.com](mailto:jelly@fabioflorey.com?subject=Jelly%20project%20inquiry).

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
