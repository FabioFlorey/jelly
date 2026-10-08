<div id="top"></div>

# Runtime Layout

**Audience:** Operators managing browser state, artifacts, backups, and cleanup.

Jelly keeps browser state, profiles, logs, artifacts, and build output outside the repository. The runtime root is configured by `config/jelly.toml` (`/data/jelly-runtime` by default). The tree below describes paths within that runtime root; compiled Cargo output is stored separately at `/data/.jelly-build`. MCP surface selection is startup configuration rather than persisted runtime state and follows the configuration precedence documented in [Configuration](../getting-started/CONFIGURATION.md).

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

Screenshot metadata records provenance, dimensions, trace linkage, and verification state. Screenshots capture Chromium-rendered page content, not the desktop. A specific target such as `css:body`, `css:main`, exact visible text, or a Jelly element ref can be captured directly.

`state/downloads.json` is the file-locked, atomically replaced lifecycle index for browser downloads and retains at most 512 recent records. Chromium is configured with GUID-named managed download files under `artifacts/downloads/`; completed files become immutable registered artifacts when finalized, and an explicit destination is a copy rather than a relocation of the managed original. In-progress records are converted to `interrupted` on a clean browser stop, when list/status/wait observes that the recorded browser launcher process has exited, at the next browser start after an unclean stop, or if the dedicated download-event connection is lost.

Browser recordings support two modes. `continuous` streams renderer frames for Jelly's shared active browser target directly into FFmpeg, so individual frame files are not written to disk. Before each frame it follows a live active-target change made by another Jelly browser session; a transient invalid/stale external target does not abort the recording, which continues against its last attached target until a valid selection is available. `steps` captures a temporary browser screenshot after relevant successful actions, records redacted tool metadata, and builds a variable-frame-rate H.264 MP4 where each action frame is held for the configured duration. Step labels resolve human-readable target names before the action so the rendered text describes the action shown without exposing internal Jelly refs; labels use concise action-in-progress wording such as `clicking Submit`. Text-entry payloads are redacted from step metadata, and recorded URLs omit credentials, query strings, and fragments. Step videos use resolution-aware Jelly overlays: honey text, black backing, concise action labels, and a small favicon watermark. On stop, Jelly registers the MP4 and manifest, then removes all temporary recording files. The desktop is never part of either recording mode. `latest.png` remains a convenience path; reliability-sensitive workflows should pass artifact IDs.

Imported Chromium session data is copied into `profiles/headed/`. Profile data and `state/oauth.json` are sensitive local state and stay outside Git.

Cargo output lives separately under `config/cargo.toml` `build.target-dir` (currently `/data/.jelly-build`).

## Destructive cleanup

> [!WARNING]
> **Both commands below delete the entire configured runtime root and the Cargo build directory.** The cleanup script stops Jelly's user services, removes persisted OAuth registrations/tokens, browser profiles and cookies, screenshots, recordings, downloads, logs, routine state, test reports, and **custom userscripts/extensions**. It also removes the Cargo build output (currently `/data/.jelly-build`) and legacy `temp/`, `logs/`, and `target/` directories. The script does not preserve Violentmonkey or other customizations. Reauthorization, browser sign-in, extension installation, and Rust rebuilds may be necessary. Do not run cleanup while you need this data.

Inspect and back up anything you intend to keep before cleaning. For example, if a Violentmonkey source directory exists, copy it **outside** the configured runtime root before running cleanup:

```bash
mkdir -p "$HOME/jelly-backup"
cp -a /data/jelly-runtime/extensions/violentmonkey-src "$HOME/jelly-backup/"
```

From the repository root, stop the managed services and delete runtime and build state:

```bash
scripts/clean-runtime.sh
```

Alternatively, run:

```bash
scripts/clean-runtime.sh --build
```

The `--build` variant additionally invokes `cargo clean` **after** deleting the build directory. It is **not** the only command that removes build output. Both commands print a completion message when the script exits successfully. The script does not delete repository source files or `.env`. Verify that any backed-up files are usable before proceeding.

<div align="right"><sub><a href="#top">🡩 Go to the top of the document</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
