<div id="top"></div>

# Runtime Layout

The repository stays source-only. Browser state, profiles, logs, screenshots, downloads, and Cargo build artifacts live outside the working tree.

Runtime data lives under:

```text
/data/jelly-runtime/
├── state/
│   └── oauth.json
│   └── oauth.json
├── profiles/
│   ├── headed/
│   └── headless/
├── logs/
│   └── actions.jsonl
├── network/
│   ├── capture.pid
│   └── requests.jsonl
├── nip-io/
│   ├── Caddyfile
│   └── caddy-data/
├── routines/
├── injections/
└── artifacts/
    ├── screenshots/
    │   └── latest.png
    └── downloads/
```

Cargo output is stored separately in:

```text
../.jelly-build/
```

Clean runtime data:

```bash
scripts/clean-runtime.sh
```

Clean runtime data and Cargo build output:

```bash
scripts/clean-runtime.sh --build
```

This keeps the repository compact and makes runtime state easy to understand and remove. `state/oauth.json` contains persisted MCP OAuth clients and access tokens with mode `0600`; cleaning the runtime removes that state and requires MCP clients to authorize again. `state/oauth.json` contains persisted MCP OAuth clients and access tokens with mode `0600`; cleaning the runtime removes that state and requires MCP clients to authorize again.

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
