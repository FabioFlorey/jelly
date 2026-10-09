<div id="top"></div>

# Configuration

**Audience:** Operators and developers configuring an existing Jelly checkout. This reference describes configuration behavior; for a first run, follow [Getting Started](./QUICKSTART.md).

Jelly separates versioned technical configuration, deployment environment values, and generated runtime state.

## Technical configuration

Technical configuration lives under `config/`. `config/jelly.toml` owns Jelly behavior and runtime layout; `config/cargo.toml` owns Cargo build output and aliases. `.cargo/config.toml` is a symlink to the latter so Cargo consumes the same versioned file.

The example below reflects the checked-in technical settings relevant to a typical startup, including the currently enabled privileged raw-CDP option. See the security note below before exposing the MCP endpoint.

```toml
[paths]
runtime_root = "/data/jelly-runtime"

[browser]
startup_timeout_secs = 30
cdp_timeout_secs = 60

[mcp]
surface = "small-surface"
persistent_session = true
raw_cdp = true
allow_cargo_fallback = false

[page]
runtime = true
snapshot_limit = 0

[diagnostics]
perf_log = false

[tests]
default_batch = "deterministic"

[ui]
animation = true
icons = true
logo = "config/brand/full-logo.txt"

[hitl.formats.telegram]
title = "Jelly"
subtitle = "Browser instrumentation for agents"
prefix = ""
suffix = ""
signature = "⏺️ Recorded with <b>Jelly</b>"
```

The Rust loader requires `config/jelly.toml` and does not read environment-variable overrides for its technical settings. The fields shown above are required, except that `mcp.connections` is optional and defaults to an empty list. Missing required sections or invalid values cause startup errors. Environment variables still configure deployment settings and secrets (see [`.env`](#env)).

> [!WARNING]
> The checked-in `raw_cdp = true` enables privileged Chromium DevTools Protocol commands for **all authenticated MCP clients**; it is not restricted to ChatGPT or a particular token. Set `[mcp].raw_cdp = false` if trusted clients do not require direct CDP access, and restart the server after changing it. See [Raw CDP trust boundary](../reference/MCP.md#raw-cdp-trust-boundary).

### MCP client connection profiles

`[mcp]` optionally accepts an array of `[[mcp.connections]]` entries. These are
**provider-independent onboarding profiles**, not individual server listeners or
per-client access-control policies. Omitting all entries preserves Jelly's
existing HTTP endpoint with static bearer-token and OAuth support. The checked-in
`config/jelly.toml` currently declares **both** profiles shown below.

```toml
[[mcp.connections]]
provider = "chatgpt"
method = "remote-http"
auth = "oauth"

[[mcp.connections]]
provider = "generic-mcp"
method = "local-http"
auth = "bearer-token"
```

Supported provider values are `chatgpt` and `generic-mcp` (the latter is the
Rust API default). New providers can be added without changing browser
primitives. Connection methods are `local-http` and `remote-http`, and client
auth methods are `bearer-token` and `oauth`. `stdio` and `none` are reserved
enum variants but are rejected at startup because those modes are not yet
implemented. All explicit profiles require a loopback MCP listener; explicit
remote HTTP profiles additionally require an HTTPS public URL. The remote HTTPS endpoint
normally forwards to Jelly's HTTP listener through a tunnel or proxy.

Profiles currently validate intended client setup choices. **They do not
restrict the server's existing accepted credential types**, provision a tunnel,
install a ChatGPT plugin, or create a stdio server. A local-only startup with the
checked-in remote ChatGPT profile and no HTTPS `JELLY_PUBLIC_URL` fails
validation. Use the hosting wizard to establish HTTPS, or remove the remote
profile before starting a local-only MCP server. `.env` still controls the
OAuth consent mode, ChatGPT DCR, public URL, and host-specific credentials.

`[hitl.formats.telegram]` controls presentation only. The runtime HITL message remains dynamic. Telegram renders the configured title in bold, escapes the dynamic message and non-HTML configured regions as HTML-safe content, and preserves trusted Telegram HTML in `signature`. Non-empty title/subtitle, body prefix/message/suffix, and signature are placed into separate message regions. Telegram credentials remain secrets in `.env`.

`runtime_root` is canonical. Jelly derives state, browser profiles, artifacts, downloads, network captures, logs, routines, injections, browser coordination files, and test-run state from that one root.

## `.env`

`.env` is local and must not be committed. It contains secrets and deployment-specific environment values such as credentials, OAuth secrets, public origins, listen addresses, hosting/tunnel settings, and HITL credentials.

`.env.example` documents that interface without containing secrets. Jelly's shell tooling reads `.env` as **data**, never with `source .env`; the setup wizard writes safely quoted entries using the Rust `jelly-maint env` utility. When loading values into an interactive shell, use `source ./scripts/config.sh; jelly_load_env`, which explicitly loads validated environment values without evaluating the contents of `.env` as code. Sourcing `config.sh` alone only defines configuration and functions, with no Rust compilation or secret loading.

Environment variables used internally to propagate per-execution context, such as trace/span identity, are runtime data rather than technical configuration.

## Runtime state

Generated state belongs below `paths.runtime_root`, not in `.env` or `config/`. This includes:

- browser state and PID/lock files;
- headed and headless browser profiles;
- screenshots, recordings, download lifecycle state/files, and artifact metadata;
- network captures and logs;
- routine continuation state;
- test-run reports.

The separation is therefore strict:

```text
config/jelly.toml   Jelly technical configuration
config/cargo.toml    Cargo/build technical configuration
.env                 secrets and deployment environment
<runtime_root>/      generated state and artifacts
```

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
