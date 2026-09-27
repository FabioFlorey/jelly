<div id="top"></div>

# Requirements

jelly has a small core dependency set plus a few optional tools used by specific integrations.

## Required

| Dependency | Purpose | Notes |
| :--- | :--- | :--- |
| **Git** | Clone/update the repository | Required by the normal source workflow |
| **Rust** | Build jelly and its binaries | Use a recent stable toolchain |
| **Cargo** | Build and run project binaries | Installed with Rust |
| **Chromium** | Browser runtime | Currently expected at `/usr/bin/chromium` |
| **systemd** | Persistent browser lifecycle | Uses user services through `systemd-run --user` |
| **bash** | Project scripts | Used by scripts under `scripts/` |
| **core shell utilities** | Common shell operations | `cp`, `mv`, `chmod`, `mktemp`, `grep`, `sed`, `awk`, `date` |
| **base64** | Screenshot decoding | Used by the screenshot tool |
| **ffmpeg / ffprobe** | Browser recording export | Encodes renderer frames into MP4 and probes step-frame dimensions for proportional branded overlays |

## Optional

| Dependency | Used by | Notes |
| :--- | :--- | :--- |
| **curl** | Telegram HITL | Required for `hitl` requests |
| **cloudflared** | Quick or fixed public MCP tunnel | Optional; the service wrapper can reuse PiLink's private binary when present |
| **Caddy** | Direct nip.io HTTPS | Required only for `JELLY_HOSTING_MODE=nip-io` |
| **python3** | nip.io public IPv4 validation | Required only for `nip-io` |
| **iproute2 (`ip`)** | nip.io LAN route discovery | Required only for `nip-io` |
| **upnpc / natpmpc** | Automatic nip.io router mappings | Optional; manual port forwarding avoids these helpers |
| **grim** | Headed screenshot fallback | Used when native Chromium capture is unavailable |
| **hyprctl** | Headed screenshot fallback | Used to determine Chromium window geometry on Hyprland |
| **Wayland / X11 session** | Headed Chromium | Not required for headless mode |

## Platform assumptions

jelly is currently developed and tested on Linux.

The current implementation assumes:

- Chromium is available at `/usr/bin/chromium`.
- `systemd --user` is available for persistent browser lifecycle management.
- Runtime data can be written to `/data/jelly-runtime`.
- Cargo build output can be written to `../.jelly-build` relative to the repository.

These are implementation assumptions rather than fundamental architectural requirements and may become configurable later.

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
Rust + Cargo
Chromium
systemd user services
bash + core shell utilities
base64
```

Everything else is feature-specific.

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
