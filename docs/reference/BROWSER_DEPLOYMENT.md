<div id="top"></div>

# Browser deployment and security choices

## Table of Contents

- [Connection modes](#connection-modes)
- [Local compatibility recommendations and explicit probes](#local-compatibility-recommendations-and-explicit-probes)
- [Security option model](#security-option-model)
- [Verified container compatibility matrix (Arch Linux, 12 October 2026)](#verified-container-compatibility-matrix-arch-linux-12-october-2026)
- [Download policy (per remote browser)](#download-policy-per-remote-browser)
- [Current implementation vs follow-up validation](#current-implementation-vs-follow-up-validation)

[Documentation index](../INDEX.md)

Jelly separates **browser identity**, **CDP transport**, **container execution**, and **artifact handling**. A remote Chromium endpoint is attached by its stable UUID (plus optional `Jelly-*` easy name). Container settings describe *how an operator launched* Chromium; they are not equivalent to properties proven by CDP.

## Connection modes

| Mode | Runtime | Configuration | Port exposure |
| --- | --- | --- | --- |
| `native` | Chromium on Jelly's host | `[browser].backend = "native"` | Native managed runtime |
| `subnet` | Operator-managed Docker/Podman Chromium | `[[browser.remotes]].connection_mode = "subnet"` | Browser IP on engine-managed `jelly-browsers` bridge; no host-port publication |
| `address` | Any reachable Chromium, local or remote | `[[browser.remotes]].connection_mode = "address"` (default) | Operator-supplied IP:port/hostname:port/WebSocket, or explicit `--publish` |

`connection_mode` is **descriptive metadata**, not a firewall or permission policy. `subnet` does not guarantee isolation from other workloads on the same bridge, and `address` does not imply Internet exposure. In both remote modes Jelly uses browser-level CDP WebSockets and one MCP process may select browser UUID/name with `browser_id`.

```bash
./scripts/dev.sh browser native
./scripts/dev.sh browser add --engine docker --profile isolated
./scripts/dev.sh browser connect --address chromium.example.internal:9222
```

The container helper owns only the containers it creates. It never starts/stops third-party services through an `address` connection. `config/browsers.toml` stores remote registrations and is excluded from version control. Jelly loads configuration on startup, so adding/removing registrations currently requires an MCP restart.

## Local compatibility recommendations and explicit probes

`./scripts/dev.sh browser detect` reports the accessible Docker/Podman engine **for the current user and socket**, including rootful/rootless status. `./scripts/dev.sh browser recommend` is read-only: it ranks **local CDP PASS** above historical Arch reference observations and leaves local FAIL/UNVERIFIED combinations selectable. `./scripts/dev.sh browser` offers those choices, a temporary browser probe and an advanced menu retaining all sandbox/restriction/profile/network settings. No daemon is started, no privileged socket access is granted and no engine is installed automatically.

`./scripts/dev.sh browser probe --engine podman --sandbox enabled --engine-security standard --profile isolated --publish 127.0.0.1:19400` explicitly builds the same image and creates temporary container/network/storage resources; it tests actual host-to-CDP reachability, cleans up and **does not register a browser**. Local PASS/FAIL evidence is cached in the private user cache, with invalidation tied to OS/kernel, engine mode/version, image/source identity and selected configuration. The probe never tests LAN-bound publication (it uses a dynamically allocated **loopback** port), shared bind-mounted downloads, MCP browser routing or effective sandbox activation. This cache is not a security attestation; non-loopback and shared-download claims remain unverified. See the [guided setup instructions](../getting-started/CONTAINER_CHROMIUM.md#guided-setup-based-on-this-host).

The availability of a rootful/rootless daemon is **not** an option that Jelly silently changes; it depends on the selected Docker/Podman socket and user privileges. Chromium's image UID 10001 remains fixed in normal use. Advanced settings never silently change an operator-selected option when a probe or browser launch fails.

## Security option model

Three **independent operator choices** are collected by `./scripts/dev.sh browser`:

| Dimension | Values | Current default | Applied where |
| --- | --- | --- | --- |
| Chromium sandbox request | `enabled`, `disabled` | `disabled` | Container Chromium entrypoint (adds `--no-sandbox` only when disabled) |
| Container restrictions | `restricted`, `standard` | `restricted` | Docker/Podman run flags. Restricted uses `no-new-privileges` and `cap-drop ALL` |
| CDP publication | none, `IPv4:PORT` | none | Engine port mapping; `0.0.0.0` can expose CDP on all host interfaces |

These are explicit *preferences*, not a composite level such as "secure"/"insecure". Jelly never silently disables a requested sandbox, relaxes restrictions, enables host port publication, or changes download mode as a fallback. The non-root Chromium process inside the image uses UID 10001; this choice is presently fixed by the image, not a configurable option.

### Diagnosis semantics

A diagnosis distinguishes:

1. **requested**: flags chosen by the operator and recorded in engine container labels;
2. **observed**: container running status, logs, and CDP endpoint reachability;
3. **unverified**: effective Chromium sandbox activation. Neither successful CDP handshakes nor process command-line flags attest the actual sandbox.

```bash
./scripts/dev.sh browser diagnose --engine docker --container jelly-chromium-1
```

This inspection is read-only. A successful startup means Chromium is reachable, **not** that the kernel sandbox is active. For deeper validation, inspect `chrome://sandbox` inside that Chromium and assess kernel namespaces/seccomp support on the runtime host. The helper does not yet provide a cross-engine, machine-verified sandbox attestation.

When sandbox-enabled startup fails, Jelly retains the requested choice, shows recent engine logs, and **suggests** actions without applying them: inspect namespace/seccomp availability, consider standard container restrictions, or explicitly request sandbox disabled. If the host cannot route to a bridge subnet, consider a loopback-only published port, a VPN/tunnel, or a separately managed endpoint. Publishing CDP beyond loopback grants extensive control over Chromium; operator-managed authentication, routing, and firewalls remain outside Jelly's responsibilities.

## Verified container compatibility matrix (Arch Linux, 12 October 2026)

This is a **host-specific observation ledger**, not a guarantee about other kernel/engine combinations. Real Chromium `154.0.8037.92` was launched through Docker `29.9.0` and Podman `6.1.3` on Arch Linux. Each of the **32 cases below was executed**: `4 engine modes x 2 container UIDs x 2 sandbox requests x 2 container restriction choices`. The baseline tests use the image's `/data/profile` directory **without a separately mounted profile volume or tmpfs**. They are **not** the complete Jelly profile/network/download matrix.

`PASS` means Chromium remained running and the host received valid CDP browser metadata from `/json/version`. `FAIL` means launch/CDP verification failed; expected errors still count as failed compatibility outcomes. These results do **not** automatically prove effective kernel sandboxing.

| Container engine mode | Chromium UID | Sandbox disabled + restricted | Disabled + standard | Sandbox enabled + restricted | Enabled + standard |
| --- | ---: | --- | --- | --- | --- |
| Docker **rootful** | 10001 | **PASS** | **PASS** | FAIL-N | FAIL-N |
| Docker **rootful** | 0 | FAIL-P | **PASS** | FAIL-P | FAIL-R |
| Docker **rootless** | 10001 | **PASS** | **PASS** | FAIL-N | FAIL-N |
| Docker **rootless** | 0 | FAIL-P | **PASS** | FAIL-P | FAIL-R |
| Podman **rootful** | 10001 | **PASS** | **PASS** | FAIL-Z | **PASS** |
| Podman **rootful** | 0 | FAIL-P | **PASS** | FAIL-P | FAIL-R |
| Podman **rootless** | 10001 | **PASS** | **PASS** | FAIL-Z | **PASS** |
| Podman **rootless** | 0 | FAIL-P | **PASS** | FAIL-P | FAIL-R |

**Totals: 32/32 baseline combinations exercised; 14 PASS, 18 FAIL, 0 unexecuted.**

Observed failure classes:

- **FAIL-N (namespace):** Docker cannot establish the namespaces needed by Chromium sandbox mode; log contains `Failed to move to new namespace ... Operation not permitted`. Seen on both rootful/rootless Docker with UID 10001 and sandbox enabled.
- **FAIL-Z (zygote):** Podman with UID 10001, sandbox enabled and `restricted` fails before CDP is ready; zygote fails (including `sys_chroot` errors in earlier runs).
- **FAIL-P (profile permissions):** UID 0 combined with `restricted` cannot create `/data/profile/.jelly-profile.lock` in the baseline image directory. This is a **profile ownership + dropped capabilities interaction**, not proof that root and `restricted` are universally incompatible.
- **FAIL-R (Chromium root rejection):** UID 0 with sandbox enabled and `standard` reaches Chromium, which rejects `Running as root without --no-sandbox`.

`restricted` requests `--security-opt no-new-privileges --cap-drop ALL`. `standard` adds neither flag; both remain subject to engine defaults. Docker/Podman container inspection confirmed separate security flags, and running Chromium process arguments confirmed `--no-sandbox` **only when `disabled` was requested**. Jelly does not silently change either choice.

### Rootless Docker is now actually tested

The rootless-Docker daemon was started **temporarily and independently** through `rootlesskit`, with a private UNIX socket and storage, without changing the installed rootful Docker daemon or adding anyone to the `docker` group. The first attempts failed because the user namespace inherited protected paths under `/run/docker` and `/run/containerd`; replacing those two symlinks **inside the private rootless mount namespace** allowed the daemon to start. It ran real Alpine and Chromium containers, including all eight combinations above. This instance reported **cgroup driver `none`**, so cgroup resource-limit enforcement was **not** verified. This is not equivalent to validating a permanent Docker-rootless service installation or its boot-time integration.

### Process UID is a separate diagnostic axis, not a Jelly CLI setting

The image's supported default is `USER jelly`, UID **10001**. UID **0** rows used the explicit test-only engine flag `--user 0:0`; Jelly's browser CLI does **not** expose a user-ID switch. With a **rootless** engine, UID 0 inside the container maps to an unprivileged user namespace on the host. Do not equate container UID 0 with host-root access.

### Profile-mode counterexample: distinct options can interact

The original `FAIL-P` rows change when the profile directory is deliberately mounted as a writable `tmpfs`, equivalent to the *profile mount semantics* of Jelly's `ephemeral` profile (`mode=1777`). These **eight additional live probes** used UID 0 and a `tmpfs` for both the Chromium profile and downloads:

| Engine | UID | Disabled + restricted | Disabled + standard | Enabled + restricted | Enabled + standard |
| --- | ---: | --- | --- | --- | --- |
| Docker rootful | 0 | **PASS** | **PASS** | FAIL-R | FAIL-R |
| Podman rootless | 0 | **PASS** | **PASS** | FAIL-R | FAIL-R |

Changing **only the profile filesystem** changes `disabled + restricted + UID 0` from FAIL-P to PASS, because UID 0 can now create the profile lock. Sandbox-enabled execution as UID 0 still fails due to Chromium's root policy. These are **direct engine probes**, not executions of the full Jelly browser-registration workflow.

### Actual sandbox checks and network evidence

The two `UID 10001 + sandbox enabled + standard` Podman PASS configurations were independently interrogated through the **real CDP WebSocket** by navigating to `chrome://sandbox`. Both Podman **rootful and rootless** reported:

- `PID namespaces: Yes`
- `Network namespaces: Yes`
- `Seccomp-BPF sandbox: Yes`
- `Seccomp-BPF sandbox supports TSYNC: Yes`
- `You are adequately sandboxed.`

This is a Chromium-reported sandbox inspection for **these two configurations only**, not a cross-engine machine-enforced attestation API. CDP reachability alone cannot establish actual sandbox activation; in particular all `disabled` PASS rows explicitly use `--no-sandbox`.

| Network/exposure probe | Live observation | Limit |
| --- | --- | --- |
| Docker rootful private bridge, sandbox disabled + restricted | Host reached a container bridge IP with **no host port published** | Containers sharing the bridge may still access CDP |
| Podman rootful private bridge, sandbox enabled + standard | Host reached a container bridge IP with **no host port published** | Bridge isolation is not client authentication |
| Podman rootless published loopback | Host reached CDP on explicitly selected `127.0.0.1:PORT` | No claim of routable rootless private bridge IP |
| Docker rootless published loopback | Host reached CDP in the temporary rootless daemon on `127.0.0.1:PORT` | Rootless service/network behavior depends on launcher and host |
| Two simultaneous Docker browsers | Distinct private IPs and browser-level CDP WebSocket IDs | These are **not** Jelly's registered `browser_id` UUIDs |

### Are the configuration dimensions independent?

**Independent inputs are not the same as independent runtime outcomes.**

| Dimension | Source of choice | What was verified |
| --- | --- | --- |
| Engine: Docker / Podman | Jelly `--engine` | Forwarded as requested; real Chromium runs on both |
| Engine privilege: rootful / rootless | Which engine socket/process is used; **not a Jelly flag** | Four real modes executed; changing daemon privilege did not implicitly alter requested sandbox/restrictions |
| Container process UID | **Image defaults to 10001; not a Jelly flag** | Direct override to UID 0 was exercised; it interacts with sandbox and profile permissions |
| Chromium sandbox: enabled / disabled | Jelly `--sandbox` | Independent forwarding; actual Chromium argument `--no-sandbox` only in disabled mode; real sandbox inspection where available |
| Engine security: restricted / standard | Jelly `--engine-security` | Independent forwarding; `no-new-privileges` and `cap-drop ALL` only for restricted |
| CDP exposure: private bridge / published bind | Jelly optional `--publish` | Host-to-bridge and explicit host-loopback routes exercised with different sandbox/restriction choices |
| Profile storage: isolated / shared / ephemeral | Jelly `--profile` | Ephemeral tmpfs probe demonstrates that profile storage can **change** which UID/restriction combinations launch; earlier separate profile tests cover persistence and file ownership |
| Image-build networking / Podman bridge DNS | Jelly build/network flags | Forwarding and earlier environment-specific tests; not part of the 32-case Chromium matrix |

The existing offline `scripts/tests/browser-policy.sh` was extended to check all **8 CLI forwarding combinations** (`docker|podman x sandbox enabled|disabled x restricted|standard`). It verifies requested labels/flags and rejects implicit UID overrides. The real-engine matrix demonstrates that **some combinations are incompatible**, which does not make the option dimensions invalid or redundant.

**Remaining separate integration gaps:** one-MCP multi-browser `browser_id` routing and per-browser authorization/state, live remote download/artifact-transfer interoperability, and automatic CI runs of the environment-specific matrix. A manual CDP/container PASS is not UAT acceptance. Follow the existing [test-governance procedure](../../tests/TAXONOMY.md#governance-decide-before-creating-tests) rather than introducing another runner.

All test containers, bridge networks and the temporary rootless-Docker daemon and data were removed. The already installed rootful Docker service was left running.

## Download policy (per remote browser)

| Mode | Storage behavior | Required settings |
| --- | --- | --- |
| `remote-only` (default) | Completed download remains remote; state/metadata available locally | `download_path` to track remote browser events |
| `shared-volume` | Host sees GUID-named files through bind mount | `download_path` + `download_host_path` |
| `transfer` | Jelly fetches completed file via an operator-selected HTTP(S) artifact bridge | `download_path` + `download_base_url`; optional `download_token` |

An HTTP bridge is permitted by explicit choice; the transfer is unencrypted and may expose downloaded content and bearer tokens. Jelly emits a runtime warning rather than silently upgrading or rejecting HTTP. MCP `download` supports `action = "transfer"` to fetch a completed remote-only download later **when an HTTP(S) bridge was configured**. Artifact transfer requires a separate server; the quickstart does not provide that server. Download mode is independent of sandbox and CDP exposure.

## Current implementation vs follow-up validation

**Implemented:** choice-aware CLI, three container profiles, network/remote connection selection, UUID/easy names, per-browser MCP routing, conditional MCP `browser_id` schema, per-browser download modes, security request labels, basic runtime diagnostics and suggestions.

**Live-tested on Arch Linux (October 2026):** Docker private bridge/no host publication and Podman rootless loopback publication, real CDP HTTP/WebSocket commands, isolated targets between Docker/Podman instances, ephemeral profile permissions, persistent volume restart with stale-lock recovery and serialized persistent-profile access. Rootless Podman's custom netavark bridge required `--disable-dns` in this headless user session (outbound DNS was verified); `--bridge-dns auto|enabled|disabled` preserves an operator override. Docker's default bridge could not reach Debian mirrors for an image build on this host; `--build-network host` allowed the build with runtime networking unchanged. Chromium's `chrome://sandbox` on Podman rootless (`sandbox=enabled`, `engine-security=standard`) reported `PID namespaces: Yes`, `Network namespaces: Yes`, `Seccomp-BPF sandbox: Yes` and `You are adequately sandboxed`. The enabled/restricted combination could not launch on either runtime, and Docker's enabled/standard combination failed on this host. These compatibility outcomes may differ by kernel and engine policy. **Not yet established by live integration tests:** HTTPS/HTTP artifact-server interoperability or dynamic container runtime matrix in CI. **Not implemented:** automatic container policy remediation, sandbox attestation, dynamic MCP configuration reload, or a managed artifact bridge.

A missing verification is reported as *unverified*, not silently upgraded to *enabled*.

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
