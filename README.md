<div id="top"></div>

<div align="center">

<img src="./assets/full-logo.png" alt="Jelly" width="760">

**Browser instrumentation for agents.**

✨ [**Quickstart**](#2-quickstart)　•　⭐ [**Architecture**](./docs/ARCHITECTURE.md)　•　⚡ [**MCP**](./docs/MCP.md)　•　💡 [**Documentation**](./docs/README.md)　•　🍯 [**Tool Index**](./.agent/tools/index.md)

📒 [Visit the project website](https://fabioflorey.github.io/jelly/)

[![Stars](https://img.shields.io/github/stars/FabioFlorey/jelly?style=flat&label=stars)](https://github.com/FabioFlorey/jelly/stargazers) [![Forks](https://img.shields.io/github/forks/FabioFlorey/jelly?style=flat&label=forks)](https://github.com/FabioFlorey/jelly/forks) [![CI](https://github.com/FabioFlorey/jelly/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/FabioFlorey/jelly/actions/workflows/ci.yml?query=branch%3Amain+event%3Apush) [![Rust 1.98.1](https://img.shields.io/badge/rust-1.98.1-000000?logo=rust)](./rust-toolchain.toml) [![License: Proprietary](https://img.shields.io/badge/license-proprietary-FFC107)](./LICENSE)

</div>

## 1. About

**Jelly** is a small Rust toolkit that gives AI agents a practical way to observe and control a real Chromium browser.

It turns browser operations such as reading pages, finding interactive elements, clicking, typing, navigating, switching tabs, uploading files, inspecting network traffic, and taking screenshots into reusable primitives that agents can discover and compose.

You do not need to know Chrome DevTools Protocol to use it. Jelly's MCP server now defaults to the `small-surface` Agent API: `browser-schema` discovers semantic operations, `browser-call` executes ordered semantic batches, and `browser-events` handles retained CDP notifications. The expanded individual-tool API is available as `large-surface`, and raw CDP stays separately opt-in.

> [!CAUTION]
> Jelly controls a real browser and can perform real actions on websites. Review routines before running them against accounts or systems you care about.

<p align="right"><sub><a href="#top">⭐ Back to top</a></sub></p>

## 2. Quickstart

Interactive setup:

```bash
git clone https://github.com/FabioFlorey/jelly.git
cd jelly
./quickstart.sh
```

The wizard checks dependencies, configures OAuth + hosting, and can install/start Jelly. See [MCP](./docs/MCP.md) for hosting modes and advanced setup.

Use `./quickstart.sh --dry-run` to preview setup, or `./quickstart.sh --check` for prerequisites only.

Or build the tools directly:

```bash
cargo build --bins
```

Open a browser and inspect a page:

```bash
cargo run --quiet --bin agent-run -- \
  open-browser https://example.com

cargo run --quiet --bin agent-run -- read-page
cargo run --quiet --bin agent-run -- snapshot-interactive
```

Act on a discovered element using the ref returned by `snapshot-interactive`:

```bash
cargo run --quiet --bin agent-run -- click '<ref>'
```

Runtime refs are document-scoped tokens such as `@eabc123-7`; legacy rollback mode uses numeric refs such as `@e7`.

Close the browser:

```bash
cargo run --quiet --bin agent-run -- close-browser
```

Discover tools without loading the full catalog:

```bash
cargo run --quiet --bin agent-discover -- capabilities
cargo run --quiet --bin agent-discover -- search "find form inputs"
cargo run --quiet --bin agent-discover -- schema type-text
```

<p align="right"><sub><a href="#top">⭐ Back to top</a></sub></p>

## 3. Documentation

<div align="center">

| Document | Description |
| :--- | :--- |
| 📒 [**Documentation Index**](./docs/README.md) | Main entry point for the documentation |
| ⚠️ [**Requirements**](./docs/REQUIREMENTS.md) | What must be installed before running Jelly |
| 💡 [**Glossary**](./docs/GLOSSARY.md) | Definitions for technical and project-specific terminology |
| ⭐ [**Architecture**](./docs/ARCHITECTURE.md) | Browser session, primitives, registry, and execution model |
| 🔑 [**Tool Discovery**](./docs/DISCOVERY.md) | Capability discovery, search, and schema loading |
| 🔆 [**Routines**](./docs/ROUTINES.md) | Guarded workflow graphs, branching, loops, and HITL continuation |
| 🍯 [**Reliability**](./docs/RELIABILITY.md) | Verification, typed failures, artifacts, timeouts, and cleanup |
| 📂 [**Runtime Layout**](./docs/RUNTIME.md) | Runtime state, build output, logs, screenshots, and cleanup |
| 🧈 [**Development**](./docs/DEVELOPMENT.md) | Project conventions, tests, and contribution rules |
| 🧪 [**Test Index**](./tests/INDEX.md) | Test-suite layout, coverage groups, batches, and executable catalog |
| ⚡ [**MCP Server**](./docs/MCP.md) | Small-surface Agent API, large-surface mode, authentication, tool mapping, and deployment boundary |
| 🍯 [**Tool Index**](./.agent/tools/index.md) | Generated reference for browser primitives and system tools |
| 🌟 [**Changelog**](./CHANGELOG.md) | Development history |

</div>

<p align="right"><sub><a href="#top">⭐ Back to top</a></sub></p>

## 4. Live demo

<div align="center">

<a href="https://fabioflorey.github.io/jelly/">
  <img src="./assets/jelly-demo.gif" alt="Jelly controlling a real Chromium browser" width="880">
</a>

<sub><strong>Jelly documenting Jelly.</strong> This video was generated by Jelly itself from a real Chromium session. After each browser action, Jelly waits for the page to settle, captures the resulting state, labels what happened, and assembles the steps into this self-documenting trace.</sub>

</div>

<p align="right"><sub><a href="#top">⭐ Back to top</a></sub></p>

## 5. Status

Jelly is under active development. Interfaces may change while the execution, discovery, and routine layers settle.

Jelly is currently distributed under a proprietary All Rights Reserved license. See [`LICENSE`](LICENSE) for the applicable terms.

**Project contact:** [jelly@fabioflorey.com](mailto:jelly@fabioflorey.com?subject=Jelly%20project%20inquiry&body=Hi%2C%0A%0AI%27m%20reaching%20out%20about%20Jelly.%0A%0ATopic%3A%20%0ADetails%3A%20%0A%0AThanks.).

<p align="right"><sub><a href="#top">⭐ Back to top</a></sub></p>
