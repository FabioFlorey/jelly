<div id="top"></div>

# Architecture

**Audience:** Developers and maintainers who need to understand system boundaries before changing Rust modules or deployment behavior.

Jelly separates browser transport, semantic browser capabilities, system/integration tools, orchestration, agent-facing discovery, MCP authentication, and a remote MCP adapter. This document explains current component relationships; see [MCP Server](../reference/MCP.md) for endpoint contracts and [Routines](../guides/ROUTINES.md) for executable workflows.

```mermaid
flowchart LR
    A[CLI / Routine / Development] --> D[Internal discovery]
    D --> P[Browser primitive registry]
    D --> S[System capability registry]
    M[MCP client] --> L[tools/list]
    L --> G[Validated Agent Tool Catalog]
    G --> P
    G --> S
    G --> N[Native Agent API builtins]
    P --> B[BrowserSession]
    N --> B
    B --> C[CDP]
    C --> E[Chromium]
    S --> X[Lifecycle / artifacts / network / routines / transports]
```

The **browser primitive registry** is the source of truth for atomic capabilities that execute against a `BrowserSession`. Each primitive declares its name, description, usage, category, arguments, validation rules, and handler.

```mermaid
flowchart LR
    R[Browser primitive registry] --> X[Execution]
    R --> V[Validation]
    R --> T[Tracing]
    R --> S[JSON schemas]
    R --> I[Generated tool index]
    R --> D[Discovery]
```

A separate **system tool registry** describes executable capabilities that should remain outside the browser primitive abstraction: browser lifecycle, screenshots and downloads, network capture, routines, and HITL. Internal discovery and the generated capability index aggregate both registries while preserving their different execution models. That internal documentation is not the MCP publication contract; the validated Agent Tool Catalog is the separate projection used by remote clients.

The Rust source is split by responsibility:

```text
src/
├── bin/
│   ├── agent-run.rs
│   ├── agent-discover.rs
│   ├── agent-open-browser.rs
│   └── ...
├── agent/
│   ├── mod.rs
│   ├── browser_call/
│   │   ├── mod.rs
│   │   ├── prepare.rs
│   │   ├── execute.rs
│   │   ├── raw.rs
│   │   ├── result.rs
│   │   └── schema.rs
│   ├── browser_events.rs
│   ├── browser_schema.rs
│   ├── builtin.rs
│   ├── catalog.rs
│   ├── catalog_build.rs
│   ├── catalog_cache.rs
│   ├── catalog_config.rs
│   ├── schema.rs
│   └── surface.rs
├── browser/
│   ├── mod.rs
│   ├── events.rs
│   ├── perf.rs
│   ├── runtime.rs
│   ├── session.rs
│   ├── target.rs
│   ├── target_manager.rs
│   └── transport.rs
├── core/
│   ├── mod.rs
│   ├── downloads.rs
│   ├── oauth.rs
│   ├── ranking.js           # pure browser-side ranking (injected by Rust)
│   ├── routines.rs
│   ├── session.rs
│   └── targets.rs
├── shell/
│   ├── mod.rs
│   ├── downloads.rs
│   ├── routines.rs
│   ├── session.rs
│   └── targets.rs
├── mcp/
│   ├── mod.rs
│   ├── dispatch.rs
│   ├── protocol.rs
│   └── system_tools.rs
├── mcp_auth/
│   ├── mod.rs
│   ├── dcr.rs
│   ├── oauth.rs
│   ├── local_approval.rs
│   ├── pages.rs
│   ├── state.rs
│   └── storage.rs
├── primitives/
│   ├── mod.rs
│   ├── input.rs
│   ├── navigation.rs
│   ├── inspect.rs
│   ├── js_helpers.rs
│   ├── verify.rs
│   ├── tabs.rs
│   ├── files.rs
│   ├── script.rs
│   ├── storage.rs
│   └── visual.rs
├── execution/
│   ├── mod.rs
│   ├── registry.rs
│   ├── named.rs
│   ├── tools.rs
│   ├── discovery.rs
│   └── tracing.rs
├── artifacts.rs
├── browser_launcher.rs
├── connection.rs
├── error.rs
├── recording.rs
├── routine.rs                 # public compatibility entrypoint
└── lib.rs
```

## Functional core and imperative shell (incremental migration)

The `src/core/` boundary holds side-effect-free decisions and data: session cache policy, download-state transitions, download materialization/collision plans, finalization/wait decisions, routine parsing, template/guard evaluation, graph validation, node planning, tool-outcome planning, logical-target label/notification decisions, OAuth authorization-code/refresh-grant validation and replay-revocation policy, and the pure browser-side semantic ranking rules in `core/ranking.js`. Core functions receive observed inputs explicitly (such as timestamps, filesystem-derived paths, or an observed tool result), rather than opening browsers, reading files, taking locks, or accessing global runtime configuration.

The `src/shell/` boundary owns the corresponding effects: CDP session caching and target synchronization in `shell/session.rs`; download tracking, persistent records, filesystem-based source selection, `fail`/`overwrite`/`uniquify` copies, artifact registration, browser process probes, and the polling clock in `shell/downloads.rs`; routine subprocess/browser execution, HITL, clock-based budgets, suspension, persistence and cleanup in `shell/routines.rs`; logical-target reconciliation, CDP session bindings, snapshot file locking and atomic persistence in `shell/targets.rs`. The routine's public entrypoint remains `jelly::routine::run_from_env` via the compatibility module `src/routine.rs`. The dependency direction is **shell → core**, never the reverse. `browser/mod.rs` continues to re-export `LogicalTarget` and the crate-internal `TargetRegistry` from the shell adapter, preserving the browser-facing interface. The public `jelly::DownloadRecord` and download API are re-exported unchanged from the library root. The MCP dispatcher calls the session adapter and translates its errors into MCP tool failures. Download file operations can be exercised with explicit temporary directories without opening the managed Jelly download root; artifact finalization continues to require its persistent store and has **not** been end-to-end tested as part of this extraction.

OAuth remains a security-sensitive adapter in `mcp_auth/oauth.rs` and `mcp_auth/storage.rs` rather than moving HTTP handlers or persistence between directories. The adapter consumes one-time authorization codes, reads the clock, generates random tokens, holds the store lock while pruning/rotating/revoking refresh grants, and persists the store. It calls side-effect-free grant/PKCE policy in `core/oauth.rs` using observed facts. **No handler, lock scope, or persistence ordering has been intentionally changed.** Security regression tests include in-memory policy checks and isolated HTTP token exchange with durable-store reload. Persist-failure fault injection remains untested. This is a functional boundary within the existing `mcp_auth` folder, not a claim that every shell adapter has been physically moved into `shell/`.

**In-page semantic ranking** is an intentional cross-language core boundary: `src/core/ranking.js` only compares normalized text and already-observed candidate fields (`match`, `disabled`, `offscreen`, document order). `src/browser/runtime.rs` injects that same source into both the optimized `search`/`resolveText` and the rollback `legacy_search_expression`; the page runtime still owns DOM and open-Shadow-DOM traversal, accessibility-name observation, layout visibility, mutation invalidation, refs and the sorted result's serialization. The runtime version is **20**, so a page with version 19 will reinstall the updated ranking code. These operations remain entirely **inside one page-side script evaluation**, with no per-candidate CDP roundtrip. The pre-migration version-19 runtime is frozen in `tests/fixtures/page-runtime-v19.js` for isolated browser comparisons. To exercise the policies without running Jelly services, see `cargo run --locked --bin jelly-maint -- test ranking` (Rust-driven Chromium with disposable profiles; no Node.js required). Single-machine microbenchmarks are only diagnostics and do not establish production latency equivalence.

This is a **partial** migration: `browser/`, `primitives/`, `mcp/`, `mcp_auth/`, and parts of `shell/routines.rs` and `shell/targets.rs` still contain application-specific decisions mixed with effects where necessary or not yet extracted. The core deliberately retains the existing semantics of legacy routine text interpolation and graph validation; neither has been redesigned. Do not relocate the entire tree solely to conform to folder names. Migration boundaries and their remaining limitations are documented here and in the isolated integration tests.

The migrated boundaries now have a **separate integration harness** at
`src/bin/jelly-fcis-probe.rs`, launched via
`scripts/check-fcis-isolated.sh`. It compiles a sanitized working-tree copy
with its own runtime and binaries, drives Chromium CDP and MCP HTTP using
loopback-only processes, and checks OAuth persistence, routine state,
download artifact finalization, and browser-session recovery. The real CDP
download tracker, persistence fault injection, and external HITL delivery
still require dedicated isolated tests; passing the harness does not
establish complete behavioral equivalence under those failure modes.

## Connection and authentication boundaries

Jelly supports two **client onboarding profiles** in `src/connection.rs`: `ChatGPT` and `GenericMcp`. A profile combines a provider name, client connection method (`local-http` or `remote-http`), and desired authentication method (`bearer-token` or `oauth`). `stdio` and unauthenticated (`none`) modes are reserved but not implemented. Connection profiles do not configure per-client authorization policies or create additional MCP listeners.

`src/bin/jelly-mcp.rs` validates explicit profiles before opening the server. The plain HTTP listener must bind to loopback; a remote profile additionally requires a public HTTPS URL, typically supplied by a hosting wrapper. The server accepts its configured static bearer token and OAuth access tokens regardless of which onboarding profile the client used.

`src/mcp_auth/` owns the OAuth and operator-approval boundary:

- `dcr.rs`, `oauth.rs`, `state.rs`, and `storage.rs` handle client registration, OAuth exchange, state, and persistence.
- `pages.rs` renders the shared Jelly shell.
- `local_approval.rs` provides loopback-only ChatGPT connection and approval pages when both paired consent and public ChatGPT DCR are enabled.

Public `/health` and `/ready` report minimal MCP service state, not browser availability. Detailed `/dashboard`, `/status`, and `/status.json` require operator access through an owner session or administrative bearer credential; ordinary OAuth access tokens are insufficient. This is a **single-owner, process-wide browser session**, not an isolated per-client browser service. The risk boundary for direct privileged CDP is documented in [MCP Server](../reference/MCP.md#raw-cdp-trust-boundary).

Most browser primitives share one persistent `BrowserSession` inside a routine. MCP browser-bound catalog entries also reuse a cached `BrowserSession` by default; `[mcp].persistent_session = false` restores one connection/attach per MCP call for rollback diagnostics. `BrowserSession` owns logical target state plus a bounded in-memory CDP notification ring and subscription registry. It enables Target discovery and flattened auto-attach for page targets, maintaining `sessionId ↔ targetId ↔ logical label` mappings in memory while persisting only logical labels/target IDs. Synchronous request waits retain notifications instead of discarding them, and `browser-events poll` uses a barrier request to drain pending frames before cursor/filter evaluation. Idle-event tests showed that this barrier model preserves the current poll-based contract, including explicit bounded-loss reporting under ring overflow, so Jelly intentionally does not add a background reader to `BrowserSession` itself.

Download lifecycle is the deliberate exception at the browser-service layer. `browser_launcher` opens a separate browser-level CDP connection, enables `Browser.downloadWillBegin`/`Browser.downloadProgress` through `Browser.setDownloadBehavior`, and persists bounded download records under a file lock so system-tool processes can list, wait, cancel, and finalize downloads independently of any one `BrowserSession`. This tracker does not feed the browser-events ring. JSON routines are guarded directed graphs: evidence is stored in context, edges can branch/loop/jump, and typed failures can select recovery paths. A graph that starts Chromium owns that browser and finalizes it on terminal completion or unhandled failure; HITL suspension preserves the session for continuation.

Executable agent entrypoints live in Cargo's native `src/bin/` layout. Standalone system tools continue to execute through the agent-tool dispatch path, which lets them manage process lifetime, files, human-intervention transport, or other concerns that do not belong in a `BrowserSession` handler. `src/agent/` owns the validated Agent Tool Catalog and native agent-facing builtins. `config/jelly.toml` `[mcp].surface` selects one validated catalog at MCP startup. The selected catalog is frozen for the process lifetime, projected into `tools/list`, and used unchanged as the `tools/call` execution allowlist. `large-surface` exposes individual browser primitives plus system tools; `small-surface` is the supported default and replaces only the browser primitives with `browser-schema`, `browser-call`, and `browser-events`. Both surfaces intentionally project the exact same ordered system-tool registry and bindings. Lifecycle/artifact/routine aggregation, HITL exposure, and operator/admin capabilities such as profile import are separate design questions and are not coupled to the browser API cutover. Internal primitive/system registries remain canonical implementation metadata rather than the remote transport surface itself. The `.agent/` tree is reserved for declarative or generated agent-facing resources such as routines and the generated tool index.

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
