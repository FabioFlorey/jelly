# Configuration

Jelly separates versioned technical configuration, deployment environment values, and generated runtime state.

## Technical configuration

Technical configuration lives under `config/`. `config/jelly.toml` owns Jelly behavior and runtime layout; `config/cargo.toml` owns Cargo build output and aliases. `.cargo/config.toml` is a symlink to the latter so Cargo consumes the same versioned file.

```toml
[paths]
runtime_root = "/data/jelly-runtime"

[browser]
startup_timeout_secs = 30
cdp_timeout_secs = 60

[mcp]
surface = "small-surface"
persistent_session = true
raw_cdp = false
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
logo = "assets/quickstart-full-logo.txt"

[hitl.formats.telegram]
title = "Jelly"
subtitle = "Browser instrumentation for agents"
prefix = ""
suffix = ""
signature = "⏺️ Recorded with <b>Jelly</b>"
```

There is no environment-variable compatibility layer for these settings. `config/jelly.toml` is required and all schema fields are required; an incomplete or invalid file fails fast.

`[hitl.formats.telegram]` controls presentation only. The runtime HITL message remains dynamic. Telegram renders the configured title in bold, escapes the dynamic message and non-HTML configured regions as HTML-safe content, and preserves trusted Telegram HTML in `signature`. Non-empty title/subtitle, body prefix/message/suffix, and signature are placed into separate message regions. Telegram credentials remain secrets in `.env`.

`runtime_root` is canonical. Jelly derives state, browser profiles, artifacts, downloads, network captures, logs, routines, injections, browser coordination files, and test-run state from that one root.

## `.env`

`.env` is local and must not be committed. It contains secrets and deployment-specific environment values such as credentials, OAuth secrets, public origins, listen addresses, hosting/tunnel settings, and HITL credentials.

`.env.example` documents that interface without containing secrets.

Environment variables used internally to propagate per-execution context, such as trace/span identity, are runtime data rather than technical configuration.

## Runtime state

Generated state belongs below `paths.runtime_root`, not in `.env` or `config/`. This includes:

- browser state and PID/lock files;
- headed and headless browser profiles;
- screenshots, recordings, downloads, and artifact metadata;
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
