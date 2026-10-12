<div id="top"></div>

# Remote Chromium (CDP)

## Table of Contents

- [Quick registration (UUID + automatic dessert names)](#quick-registration-uuid-automatic-dessert-names)
- [Important limitations](#important-limitations)

[Documentation index](../INDEX.md)

**Audience:** Advanced operators connecting existing Chromium browsers that Jelly does not start or stop.

CDP is the Chrome DevTools Protocol. Begin with [Getting Started](../getting-started/QUICKSTART.md) if you only need one local browser; this guide is for independently hosted browsers and their downloads.

Jelly can attach to **operator-managed** Chromium instances over browser-level CDP WebSockets. Docker and Podman are not integration dependencies: the operator starts Chromium, handles network isolation, and supplies the browser WebSocket URL.

## Quick registration (UUID + automatic dessert names)

Build or run the helper:

```bash
cargo run --bin jelly-browser-connect -- add http://127.0.0.1:9222 --mode remote-only
cargo run --bin jelly-browser-connect -- add ws://127.0.0.1:9223/devtools/browser/YOUR-ID --mode shared-volume --remote-path /browser-downloads --host-path /mnt/browser-downloads
cargo run --bin jelly-browser-connect -- list
cargo run --bin jelly-browser-connect -- remove Jelly-Biscuit
```

The helper discovers the browser-level WebSocket through Chromium's `/json/version` endpoint for HTTP URLs, verifies that the resulting CDP WebSocket is reachable, and stores registrations in `config/browsers.toml`. It assigns a stable UUID v4 and the next unused dessert name from 208 distinct sweets arranged in eight A-Z rounds: Jelly-Applepie, Jelly-Biscuit, Jelly-Cannoli, Jelly-Dorayaki, through Jelly-Zabaione, then Jelly-Alfajores for the next round. New additions include Jelly-Chiacchiere, Jelly-Jellybeans, and Jelly-Candies. Numbered suffixes are used only after the catalog is exhausted. The names draw from traditional sweets across countries; brand names are not used. The helper switches `config/jelly.toml` to `remote-cdp` on first successful registration. **Restart the Jelly MCP service after adding/removing browsers**: configuration is loaded once at startup. The helper does not start or stop containers or Chromium instances.

For an externally hosted service use `./scripts/dev.sh browser connect --address HOST:PORT` (or supply a WebSocket URL); registration uses `connection_mode = "address"`. A container on Jelly's dedicated subnet is recorded as `connection_mode = "subnet"`. These values describe topology, not a security guarantee. For optional artifact transfer use `--mode transfer --remote-path /browser-downloads --download-url https://your-artifact-bridge/files`. The operator still has to expose a protected artifact endpoint; the helper will not create one. It does not persist authentication secrets from command-line arguments.


Set `backend = "remote-cdp"` in `config/jelly.toml` and configure one or more endpoints:

```toml
[browser]
backend = "remote-cdp"
startup_timeout_secs = 30
cdp_timeout_secs = 60

[[browser.remotes]]
id = "8f3a0000-1111-4222-8333-44444444c921"
name = "Jelly-Applepie"
endpoint = "ws://127.0.0.1:9222/devtools/browser/REPLACE_ME"
connection_mode = "address" # or "subnet" for a managed bridge
# Remote download tracking path (required for tracked remote downloads):
download_path = "/browser-downloads"
# download_mode = "remote-only"   # default; no automatic local copy
# download_mode = "shared-volume"
# download_host_path = "/mnt/browser-downloads/worker-a"
# download_mode = "transfer"      # fetch via HTTP(S) on wait
# Only required for HTTP(S) transfers:
# download_base_url = "https://artifact-proxy.internal/worker-a/files"
# download_token = "REPLACE_WITH_SECRET"

[[browser.remotes]]
id = "2b7e0000-1111-4222-8333-44444444a104"
name = "Jelly-Biscuit"
endpoint = "ws://127.0.0.1:9223/devtools/browser/REPLACE_ME"
```

For CLI processes you can select each registered browser by UUID or easy name; the MCP server supports per-call `browser_id` without requiring a separate process:

```bash
JELLY_BROWSER_ID=Jelly-Applepie cargo run --bin agent-run -- open-browser
JELLY_BROWSER_ID=Jelly-Biscuit cargo run --bin agent-run -- open-browser
```

Each remote browser has an immutable UUID (`id`) and an optional human-readable alias (`name`). Calls can select either; state and caches always use the UUID. Aliases must be unique. Each remote browser uses a distinct runtime tree under `/data/jelly-runtime/remote/<id>`, including target labels and artifact metadata. One MCP instance can select a configured browser by passing `browser_id` with browser tool arguments, for example `{"browser_id":"Jelly-Applepie","calls":[{"call":{"jelly":"read-page"}}]}`. Sessions are cached independently by ID. This routing has not yet been verified with simultaneous live Chromium processes.

`close-browser` on a remote worker only cleans up Jelly's local state; it does **not** close remote Chromium. `open-browser` verifies that the provided WebSocket is reachable and registers its endpoint. Browser sessions can also connect directly through the named endpoint without invoking `open-browser` first.

## Important limitations

- Remote browser profile storage is owned by the remote browser operator.
- Download policies per browser: `download_mode = "remote-only"` (default) records completion without copying; `download_mode = "shared-volume"` uses a host-mounted directory configured as `download_host_path`; `download_mode = "transfer"` fetches completed files over HTTP or HTTPS, as explicitly selected by the operator. Every remote browser must configure `download_path` inside Chromium. For remote-only, explicit MCP `download` with `action = "transfer"` fetches the file later when HTTP(S) settings are supplied. The transfer mode requires `curl` on Jelly and an operator-managed file bridge: configure `download_path` on the remote Chromium filesystem and `download_base_url` as an HTTP(S) base URL serving `GET /<GUID>` (where the GUID names the completed file) with raw bytes. Set `download_token` if the bridge requires a bearer token. Jelly subscribes to CDP download events, stores per-browser progress, and downloads completed GUID-named files to its local artifact directory when materializing them. HTTP artifact transfers are unencrypted; Jelly warns about exposing content and bearer tokens in transit. The bridge must not serve incomplete files and must restrict access to that worker's download directory. Container orchestration and the bridge service remain operator responsibilities. Download tracking requires `download_path`; HTTPS transfer specifically additionally requires `download_base_url`. The `remote-only` policy can record completed downloads without a local file.
- The selected remote browser must already expose at least one page target.
- MCP tools accept a per-request `browser_id`. Native CLI processes still use `JELLY_BROWSER_ID` for their selection; do not change it mid-process.
- CDP provides broad browser control and access to authenticated sessions. Loopback, VPNs and authenticated tunnels are available protection choices; Jelly does not prohibit an explicitly operator-selected public interface. The operator must evaluate exposure and enforce any required network and authentication controls. This choice is independent of Chromium sandbox configuration.
- Do not share a browser CDP endpoint between Jelly workers expecting isolated state.
- Tests must cover remote connection, disconnect/recovery, target isolation and download behavior before declaring feature parity with native browser mode.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
