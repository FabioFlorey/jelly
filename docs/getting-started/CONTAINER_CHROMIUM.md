# Chromium connections: native, subnet, or IP:PORT

[Documentation index](../INDEX.md)

Choose one of three connection strategies from `./scripts/dev.sh browser`:

- **Native:** run Chromium on the Jelly host (`./scripts/dev.sh browser native`).
- **Subnet:** start Docker/Podman Chromium in the dedicated `jelly-browsers` bridge; no published CDP ports.
- **Address:** connect to any reachable Chromium endpoint at `IP:PORT`, `hostname:PORT`, or a browser-level `ws(s)://` URL. Jelly will not manage this browser's lifecycle.

For example, to register an existing Chromium on another host or behind an operator-managed tunnel:

```bash
./scripts/dev.sh browser connect --address 192.0.2.10:9222
# or
./scripts/dev.sh browser connect --address chromium.internal:9222
# or
./scripts/dev.sh browser connect --address 'wss://chromium.internal/devtools/browser/UUID'
```

The external mode checks connectivity and generates an automatic UUID and dessert name. It does **not** launch a container. An explicitly selected external/public CDP address is permitted, but it exposes powerful browser controls to anyone with network access. Private VPNs, authenticated tunnels, reverse proxies and firewall rules are alternatives, not silent requirements.

On Linux, start from the Jelly repository:

```bash
./scripts/dev.sh browser
```

Or run the noninteractive command:

```bash
./scripts/dev.sh browser add --engine docker --profile isolated
# or
./scripts/dev.sh browser add --engine podman --profile ephemeral
```

Each invocation creates **another Chromium container** and registers it with Jelly using an immutable UUID and an automatic Jelly-* dessert name. The helper builds the local `containers/chromium/Containerfile`, creates (if needed) the dedicated **`jelly-browsers` bridge network**, obtains the container's bridge IP, checks `/json/version`, and registers the browser-level CDP WebSocket. By default no CDP port is published to the host or LAN; operators can choose `--publish IPv4:PORT`. The container network subnet is allocated by Docker/Podman, avoiding hard-coded ranges that could overlap an existing network. Chromium 154+ may bind DevTools only to its loopback interface; the image uses an unprivileged `socat` forwarder **inside the container namespace** from port 9222 to Chromium's loopback CDP port 9223. This does not publish a host port.

To inspect or stop an individual container:

```bash
./scripts/dev.sh browser list --engine docker
./scripts/dev.sh browser stop --engine docker --container jelly-chromium-1
```

Stopping the container does not delete its Jelly registration. Use `cargo run --bin jelly-browser-connect -- remove Jelly-Applepie` to remove it from Jelly. Adding or removing browser registrations requires restarting the Jelly MCP service to reload its static configuration.

## Guided setup based on this host

The existing `./scripts/dev.sh browser` menu first detects which Docker/Podman daemon or rootless socket is **accessible to the current user**. It never installs a container engine, starts a daemon, runs `sudo`, or adds the user to the root-equivalent `docker` group. If neither engine is accessible, **native Chromium** and **external CDP** remain available.

```bash
./scripts/dev.sh browser detect       # read-only availability and rootless/rootful status
./scripts/dev.sh browser recommend    # read-only local CDP results, then historical reference candidates
./scripts/dev.sh browser              # interactive guided menu
```

**Local PASS** means a previously requested temporary Chromium container was running and its CDP `/json/version` returned a browser-level WebSocket URL. It is **not** sandbox attestation, an MCP end-to-end pass, or a guarantee that downloaded files and other browser profiles work. A **reference PASS** comes only from the [Arch compatibility matrix](../reference/BROWSER_DEPLOYMENT.md#verified-container-compatibility-matrix-arch-linux-12-october-2026): it is never presented as a local PASS.

Users can test a configuration before registering a browser:

```bash
./scripts/dev.sh browser probe --engine podman --sandbox enabled --engine-security standard \\
  --profile isolated --publish 127.0.0.1:19400
```

The explicit probe builds the local image, creates a temporary private bridge and disposable profile/download volumes (or tmpfs for `ephemeral`), starts Chromium, checks CDP and removes its container, bridge and volumes. **It does not update browser registrations or start/stop Jelly services.** For a published configuration it uses a **random loopback-only port** even if an operator selected a different address: this validates loopback transport, **not LAN exposure or a specific port**. Probes of `shared` mode do **not** verify its bind-mounted downloads. Rootless Podman requires the operator to select `--publish 127.0.0.1:PORT` explicitly; Jelly does not silently substitute publication for a selected subnet connection.

Only real probe PASS/FAIL results are cached privately under `${XDG_CACHE_HOME:-~/.cache}/jelly/browser-probes/`. The cache key covers host OS/kernel, engine version and rootless/rootful mode, source Dockerfile/entrypoint and built image identity, sandbox/restrictions, profile, build network, bridge DNS and **subnet versus loopback** topology. Changes invalidate prior evidence. Shared-volume semantics or publication beyond `127.0.0.1` are never marked as fully verified by the CDP probe. Cache data is a local convenience, not a machine-verifiable security attestation.

The menu lists **local PASS choices first**, followed by reference-PASS candidates, locally failing choices and other configurations. All other combinations stay accessible via **Advanced settings**; a local FAIL produces warnings, **not** an automatic sandbox/restriction fallback. When choosing a suggestion, the user still selects the profile, publication and build network explicitly and confirms before a real browser is created.

## Profiles

| Profile | Command flag | Browser profile | Download storage |
| --- | --- | --- | --- |
| isolated | `--profile isolated` | Persistent engine volume | Persistent private engine volume; Jelly tracks but does not copy files |
| shared | `--profile shared` | Persistent engine volume | Bind-mounted `.jelly-downloads/<container>/`, readable from the host |
| ephemeral | `--profile ephemeral` | Temporary tmpfs | Temporary tmpfs, deleted when the container is removed |

For `shared`, the download directory must appear owned by UID 10001 **inside the container's user namespace**. Under rootful Docker/Podman, that normally means `sudo chown 10001:10001 <directory>`; under rootless Podman, UID 10001 maps to a subordinate host UID, so use `podman unshare chown 10001:10001 <directory>`. The helper checks the correct namespace and prints the applicable ownership command rather than making the directory world-writable. Host reads also depend on the permissions of files created by Chromium; this was validated with an ordinary readable file on the test host. The other profiles require no host download-directory permissions. Persistent profiles use a nonblocking volume lock to prevent concurrent browser instances and to discard stale Chromium `Singleton*` markers when a stopped container is recreated. Ephemeral profile/download mounts use sticky `tmpfs` permissions (`mode=1777`) so the unprivileged Chromium UID can write them; they remain private to the container mount namespace.

### Optional image build networking

A container image build must be able to download Debian/Chromium packages. If Docker bridge networking lacks outbound connectivity on your host, explicitly select `--build-network host`:

```bash
./scripts/dev.sh browser add --engine docker --profile isolated --build-network host
```

This choice affects **image build RUN steps only**, not Chromium's runtime networking or the selected CDP publication policy. The default is `--build-network default`; Jelly does not silently switch to host networking. A rootful Docker socket may require administrator access: being able to run `sudo docker ...` does not mean the current user has permission to connect directly. Jelly does not automatically add users to the root-equivalent `docker` group.

## Networking and security constraints

**Direct host-to-container bridge-IP connectivity is expected on Linux rootful Docker/Podman when ports are not published.** Rootless Podman and Docker Desktop do not guarantee that the host can route to private bridge IPs. The helper offers an explicit `--publish IPv4:PORT` alternative for these environments and tests actual HTTP connectivity before registering a browser. In a headless rootless Podman session with no user systemd bus, creating a custom bridge with the `aardvark-dns` plugin can fail. `--bridge-dns auto` (default) selects `--disable-dns` for that case **with a visible notice**; outbound DNS and HTTP were confirmed to work, but container-name DNS on that bridge is unavailable. The operator can choose `--bridge-dns enabled` or `--bridge-dns disabled` explicitly instead. This is a DNS-networking choice, not a sandbox or host-port security fallback. For rootless/Desktop, operators can choose explicit host publication, a VPN/tunnel, or an externally managed CDP URL. Jelly reports the implications and validates reachability without imposing a security configuration.

The helper allows independent security choices: `--sandbox enabled|disabled`, `--engine-security restricted|standard`, and optional `--publish IPv4:PORT` to bind CDP explicitly (including LAN-facing addresses if the operator chooses). Defaults are `disabled`, `restricted`, and **no publication**. The container uses UID 10001. Chromium sandbox compatibility depends on the host kernel and container restrictions; selecting `enabled` does not guarantee it can start. CDP publication, particularly beyond loopback, grants broad remote browser control: protect it appropriately for your environment. These are informed user choices, not blanket prohibitions. The bridge network is private in the sense of having no host port publication, but other workloads attached to it could still contact CDP; do not attach untrusted containers. Limit the network to Jelly-managed browser containers and control access to the host.

This quickstart does **not** provide the HTTP(S) artifact bridge for remote `transfer` download mode. For that, configure an operator-managed HTTP(S) file service as described in [Remote CDP](../guides/REMOTE_CDP.md). The `isolated` and `ephemeral` quickstart profiles use `remote-only` downloads and the `shared` profile uses `shared-volume`.

## Security diagnosis

The interactive menu asks separately for Chromium sandbox, engine restrictions and optional CDP bind address. For non-interactive use:

```bash
./scripts/dev.sh browser add --engine docker --profile isolated --sandbox enabled --engine-security standard --publish 127.0.0.1:9222
./scripts/dev.sh browser diagnose --engine docker --container jelly-chromium-1
```

The diagnostic report distinguishes requested settings from observed container state and leaves effective Chromium sandbox activation **unverified**. To inspect Chromium's own sandbox report, open `chrome://sandbox` in that browser. On the tested Arch Linux host, **Podman rootless + sandbox enabled + standard restrictions** reported active namespaces and Seccomp-BPF; **Docker (standard/restricted)** and **Podman (restricted)** could not launch Chromium with sandbox enabled. Those are host-specific observations, not universal compatibility promises. Failure output includes engine logs and actionable suggestions without automatic fallback. For the complete compatibility/verification contract, see [Browser deployment and security choices](../reference/BROWSER_DEPLOYMENT.md).

## Existing native Chromium

Select the local native browser backend via:

```bash
./scripts/dev.sh browser native
```

The helper only changes the backend setting; it does not terminate running containers or automatically restart Jelly MCP.
