<div id="top"></div>

# Requirements

**Audience:** Developers and operators preparing to install Jelly on Linux. For an end-to-end first run, follow [Getting Started](./QUICKSTART.md).

Jelly has a small core dependency set plus a few optional tools used by specific integrations. The small-surface MCP Agent API is the default surface and requires no additional runtime dependency beyond the normal Jelly stack; large-surface remains available as the explicit individual-tool compatibility mode.

## Required

| Dependency | Purpose | Notes |
| :--- | :--- | :--- |
| **Git** | Clone/update the repository | Required by the normal source workflow |
| **Rust** | Build Jelly and its binaries | Requires Rust 1.98; `rust-toolchain.toml` currently pins 1.98.1 |
| **Cargo** | Build and run project binaries | Installed with Rust |
| **Chromium** | Browser runtime | Currently expected at `/usr/bin/chromium` |
| **systemd** | Persistent browser lifecycle | Requires `systemctl`, `systemd-run`, and a working user manager |
| **bash** | Project scripts | Used by scripts under `scripts/` |
| **base64** | Setup/auth helper | Required by the quickstart prerequisite check |
| **core shell utilities** | Common shell operations | `cp`, `mv`, `chmod`, `mktemp`, `grep`, `sed`, `awk`, `date` |

## Optional

| Dependency | Used by | Notes |
| :--- | :--- | :--- |
| **curl** | Selected integrations and setup flows | Used by Telegram HITL, paired OAuth owner-status checks, and nip.io public IPv4 discovery |
| **cloudflared** | Quick or fixed public MCP tunnel | Required for `quick-tunnel` and `cloudflare-fixed`; the service wrapper can reuse PiLink's private binary when present |
| **Caddy** | Direct nip.io HTTPS | Required only for `JELLY_HOSTING_MODE=nip-io` |
| **iproute2 (`ip`)** | nip.io LAN route discovery | Required only for `nip-io` |
| **upnpc / natpmpc** | Automatic nip.io router mappings | Required only for automatic nip.io router mapping; manual port forwarding avoids these helpers |
| **ffmpeg / ffprobe** | Browser recording export | Required only when exporting browser recordings; encodes frames into MP4 and probes step-frame dimensions |
| **Wayland / X11 session** | Headed Chromium | Not required for headless mode |

## Platform assumptions

Jelly is currently developed and tested on Linux.

The current implementation assumes:

- Chromium is available at `/usr/bin/chromium`.
- `systemd --user` is available for persistent browser lifecycle management.
- Runtime data can be written to `/data/jelly-runtime`.
- Cargo build output can be written to `/data/.jelly-build`, as specified by `config/cargo.toml` (`[build].target-dir`).

These are implementation assumptions rather than fundamental architectural requirements and may become configurable later.

## Development and verification tools

Some repository regression and performance scripts use additional command-line tools that are not required for normal Jelly runtime:

| Dependency | Used by | Notes |
| :--- | :--- | :--- |
| **jq** | Regression and browser verification scripts | Used to inspect and assert JSON output |
| **curl** | MCP lifecycle and verification scripts | Used to call local MCP/health endpoints |
| **util-linux (`flock`)** | Scenario test suite | Serializes executable suite runs that share Jelly browser/service state |
| **Python 3.11+** | Documentation checks and shell configuration helpers | Runs `scripts/check-docs.py` (uses `tomllib`) and setup helpers |

## Optional integration configuration

Telegram support requires the following environment variables:

```text
JELLY_TELEGRAM_BOT_TOKEN
JELLY_TELEGRAM_CHAT_ID
```

They can be provided through the environment or the project `.env` file.

## Minimum setup

For headless browser automation, the practical minimum is:

```text
Git
Rust 1.98 + Cargo
Chromium
systemd user services (`systemctl` + `systemd-run`)
bash + base64 + core shell utilities
```

Everything else is feature-specific. The MCP quick-tunnel setup additionally requires `cloudflared`; local-only startup must not retain a `remote-http` connection profile without a configured HTTPS public origin. See [Getting Started](./QUICKSTART.md) for a verifiable setup procedure.

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
