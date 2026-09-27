#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${JELLY_CLOUDFLARE_TUNNEL_TOKEN:-}" ]]; then
  echo "JELLY_CLOUDFLARE_TUNNEL_TOKEN is not set" >&2
  exit 2
fi

cloudflared="${JELLY_CLOUDFLARED_BIN:-}"
if [[ -z "$cloudflared" ]]; then
  if command -v cloudflared >/dev/null 2>&1; then
    cloudflared="$(command -v cloudflared)"
  elif [[ -x "$HOME/.config/pilink/bin/cloudflared" ]]; then
    cloudflared="$HOME/.config/pilink/bin/cloudflared"
  else
    echo "cloudflared not found; install it or set JELLY_CLOUDFLARED_BIN" >&2
    exit 2
  fi
fi

export TUNNEL_TOKEN="$JELLY_CLOUDFLARE_TUNNEL_TOKEN"
exec "$cloudflared" tunnel --no-autoupdate run
