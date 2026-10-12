<div id="top"></div>

# Jelly Documentation

## Table of Contents

- [Getting started](#getting-started)
- [Guides](#guides)
- [Reference](#reference)
- [Development and architecture](#development-and-architecture)

**New to Jelly?** Start with the [plain-language website Getting Started guide](https://fabioflorey.github.io/jelly/getting-started.html), then [see browser and routine examples](https://fabioflorey.github.io/jelly/examples.html). The Markdown [Getting Started](./getting-started/QUICKSTART.md) gives the complete CLI workflow.

**Using this documentation:** Getting started explains setup, guides cover real workflows, references record exact interfaces and security boundaries, and development/test pages are written for maintainers rather than general users.

## Getting started

- [Getting Started](./getting-started/QUICKSTART.md): Installation, service checks, and a first browser operation.
- [Requirements](./getting-started/REQUIREMENTS.md): System prerequisites and optional integrations.
- [Configuration](./getting-started/CONFIGURATION.md): Application settings, credentials, and deployment profiles.
- [Container Chromium](./getting-started/CONTAINER_CHROMIUM.md): Docker/Podman quickstart, native/subnet/address options and browser profiles.

## Guides

- [Agent Tool-Selection Playbook](./guides/AGENT_PLAYBOOK.md): Choosing tools and verifying results.
- [Reliability](./guides/RELIABILITY.md): Errors, references, artifacts, timeouts, and recovery.
- [Remote CDP](./guides/REMOTE_CDP.md): UUID/easy-name registration and remote download modes.
- [Routines](./guides/ROUTINES.md): Browser workflows, branching, and human approval.

## Reference

- [CLI Processes Diagrams](./reference/diagrams/DEV.SH.md): Mermaid flowcharts for quickstart, connections, browsers, operations, and testing.
- [MCP Server](./reference/MCP.md): Protocol endpoints, tools, authentication, and hosting.
- [Capability Discovery](./reference/DISCOVERY.md): Internal and MCP-facing discovery contracts.
- [Runtime Layout](./reference/RUNTIME.md): Profiles, artifacts, services, and destructive cleanup.
- [Browser Deployment & Security](./reference/BROWSER_DEPLOYMENT.md): Model of requested, observed and unverified security choices, transport and download policy.
- [Glossary](./reference/GLOSSARY.md): Terminology.
- [Internal Capability Index](../.agent/tools/index.md): Generated primitive and tool metadata.

## Development and architecture

- [Development](./development/DEVELOPMENT.md): Local checks, tests, and developer tooling.
- [Architecture](./architecture/ARCHITECTURE.md): Browser session design and code boundaries.
- [MCP Surface Benchmarks](./development/MCP_SURFACE_BENCHMARKS.md): Tool-schema and runtime measurements.
- [Agent Guidance Evaluation](./development/AGENT_EVALUATION.md): Routing scenarios and evaluation criteria.
- [Test Index](../tests/INDEX.md): Test coverage and groups.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
