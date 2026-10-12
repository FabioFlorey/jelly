#!/bin/sh
set -eu
set -- chromium --headless=new --no-first-run --no-default-browser-check --disable-dev-shm-usage --remote-debugging-address=127.0.0.1 --remote-debugging-port=9223 --user-data-dir=/data/profile "$@"
case "${JELLY_CHROMIUM_SANDBOX:-disabled}" in
  enabled) ;;
  disabled) set -- "$@" --no-sandbox ;;
  *) echo 'Invalid JELLY_CHROMIUM_SANDBOX (enabled|disabled)' >&2; exit 2 ;;
esac
# Persistent volumes retain Chromium's Singleton* links when a container is removed.
# Hold a nonblocking volume lock before clearing only those stale process markers.
# Never unlock a profile that is still in use by another Jelly container.
exec 9>/data/profile/.jelly-profile.lock
if ! flock -n 9; then
  echo 'Chromium profile is already in use by another container' >&2
  exit 1
fi
rm -f /data/profile/SingletonLock /data/profile/SingletonCookie /data/profile/SingletonSocket

# Chromium 154 binds CDP to loopback despite --remote-debugging-address=0.0.0.0.
# Expose only inside the container's network namespace, not on the host.
socat TCP-LISTEN:9222,bind=0.0.0.0,reuseaddr,fork TCP:127.0.0.1:9223 &
exec "$@" about:blank
