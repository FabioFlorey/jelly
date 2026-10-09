#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
USER_UNITS="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

systemctl --user disable --now jelly-cloudflared.service >/dev/null 2>&1 || true
systemctl --user disable --now jelly-mcp.service >/dev/null 2>&1 || true
rm -f "$USER_UNITS/jelly-cloudflared.service" "$USER_UNITS/jelly-mcp.service"
systemctl --user daemon-reload
printf 'jelly MCP services removed.\n'
