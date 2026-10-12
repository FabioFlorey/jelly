<div id="top"></div>

# Runtime Layout

## Table of Contents

- [Where Jelly stores data](#where-jelly-stores-data)
- [Artifacts and downloads](#artifacts-and-downloads)
- [Browser recordings](#browser-recordings)
- [Service operations](#service-operations)
- [Destructive cleanup](#destructive-cleanup)

**Audience:** Operators managing browser state, artifacts, backups, and cleanup.

## Where Jelly stores data

Jelly stores browser state, profiles, logs, artifacts, and build output outside the repository. The runtime root is defined in `config/jelly.toml` (default: `/data/jelly-runtime`).

In remote-CDP mode, each browser has a separate `<runtime_root>/remote/<browser-uuid>/` tree for labels, downloads, and artifacts. Jelly does not own the external Chromium process or its remote profile. Native browser state uses the root-level paths shown below.

Cargo compilation output is stored separately at `/data/.jelly-build`. The MCP tool surface is selected at startup, not stored as runtime state. For configuration precedence, see [Configuration](../getting-started/CONFIGURATION.md); for browser ownership, see [Browser deployment](./BROWSER_DEPLOYMENT.md).

```text
<runtime_root>/
├── state/
│   ├── oauth.json
│   ├── downloads.json
│   └── ... browser/CDP state
├── profiles/
│   ├── headed/
│   └── headless/
├── logs/
│   ├── actions.jsonl
│   └── perf.jsonl        # only when [diagnostics].perf_log is enabled
├── network/
│   └── requests.jsonl
├── routines/
├── injections/
├── test-runs/            # scenario-suite reports/logs
├── test-suite.lock       # executable-suite concurrency lock
└── artifacts/
    ├── metadata/
    │   └── artifact-*.json
    ├── screenshots/
    │   ├── latest.png
    │   └── registered/
    │       └── artifact-*.png
    ├── recordings/
    │   ├── active.json
    │   ├── recording-*/
    │   │   ├── manifest.json
    │   │   └── recording.mp4
    │   └── registered/
    │       ├── artifact-*.mp4
    │       └── artifact-*.json
    └── downloads/
        ├── <download-guid>
        └── registered/
```

## Artifacts and downloads

Screenshot metadata records provenance, dimensions, trace linkage, and verification state. Screenshots capture Chromium-rendered page content, not the desktop. A specific target such as `css:body`, `css:main`, exact visible text, or a Jelly element ref can be captured directly.

In remote mode, download records can complete without a Jelly-local file (`remote-only`), use a host-mounted GUID file (`shared-volume`), or be materialized through an explicitly configured HTTP(S) artifact bridge (`transfer`). The artifact bridge is not bundled with the container quickstart.

`state/downloads.json` is the file-locked, atomically replaced lifecycle index for browser downloads and
retains at most 512 recent records. Chromium is configured with GUID-named managed download files under
`artifacts/downloads/`; completed files become immutable registered artifacts when finalized, and an
explicit destination is a copy rather than a relocation of the managed original.

In-progress records are converted to `interrupted` on a clean browser stop, when list/status/wait
observes that the recorded browser launcher process has exited, at the next browser start after an
unclean stop, or if the dedicated download-event connection is lost.

## Browser recordings

Jelly offers two recording modes:

- **`continuous`:** Streams frames from the active browser target directly to FFmpeg without saving individual image files. If another Jelly session changes the active target, recording follows the change. When an external target is temporarily invalid or stale, recording continues using the last valid target until it can switch.
- **`steps`:** Captures screenshots after relevant successful actions, stores redacted action metadata, and produces a variable-frame-rate H.264 MP4. Each screenshot remains on screen for the configured duration.

For step recordings, Jelly resolves human-readable target names before actions. Labels describe what the viewer sees (for example, `clicking Submit`) without exposing internal references. Text-entry values are redacted, and recorded URLs omit credentials, queries, and fragments.

Both modes use Chromium-rendered content, **never the desktop**. Step videos also use Jelly overlays and a favicon watermark. On stop, Jelly registers the MP4 and manifest, then deletes temporary recording files. Use artifact IDs instead of `latest.png` when a workflow needs reliable evidence.

Imported Chromium session data is copied into `profiles/headed/`. Profile data and `state/oauth.json` are sensitive local state and stay outside Git.

Cargo output lives separately under `config/cargo.toml` `build.target-dir` (currently `/data/.jelly-build`).

## Service operations

From the repository root:

| Command | Effect |
| --- | --- |
| `./scripts/dev.sh status` | Inspect installed service and MCP status. |
| `./scripts/dev.sh start` | Start the MCP service and the native browser service only when native mode is selected. |
| `./scripts/dev.sh stop` | Stop MCP; stop the native browser service only when native mode is selected. Externally managed Chromium processes continue running. |
| `./scripts/dev.sh logs` | Show recent MCP journal entries. |
| `./scripts/dev.sh uninstall` | Preview removal of MCP/tunnel user service units. |
| `./scripts/dev.sh uninstall --yes` | Stop, disable and remove MCP/tunnel unit files. Browser profiles and runtime data are not removed. |

`install`, `start`, and `stop` affect the installed user services. Use them only
when you intend to change the running Jelly environment.

## Destructive cleanup

> [!WARNING]
> **Both commands below delete the entire configured runtime root and the Cargo build directory.** The cleanup script stops Jelly's user services, removes persisted OAuth registrations/tokens, browser profiles and cookies, screenshots, recordings, downloads, logs, routine state, test reports, and **custom userscripts/extensions**. It also removes the Cargo build output (currently `/data/.jelly-build`) and legacy `temp/`, `logs/`, and `target/` directories. The script does not preserve Violentmonkey or other customizations. Reauthorization, browser sign-in, extension installation, and Rust rebuilds may be necessary. Do not run cleanup while you need this data.

Inspect and back up anything you intend to keep before cleaning. For example, if a Violentmonkey source directory exists, copy it **outside** the configured runtime root before running cleanup:

```bash
mkdir -p "$HOME/jelly-backup"
cp -a /data/jelly-runtime/extensions/violentmonkey-src "$HOME/jelly-backup/"
```

From the repository root, preview destructive cleanup with `./scripts/dev.sh clean`. To stop the managed services and delete runtime and build state explicitly:

```bash
./scripts/dev.sh clean --yes
```

Alternatively, run:

```bash
./scripts/dev.sh clean --build --yes
```

The `--build` variant additionally invokes `cargo clean` **after** deleting the build directory. It is **not** the only command that removes build output. Both commands print a completion message when the script exits successfully. The script does not delete repository source files or `.env`. Verify that any backed-up files are usable before proceeding.

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
