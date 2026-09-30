<div id="top"></div>

# Runtime Layout

Jelly keeps browser state, profiles, logs, artifacts, and build output outside the repository.

```text
/data/jelly-runtime/
├── state/
│   ├── oauth.json
│   └── ... browser/CDP state
├── profiles/
│   ├── headed/
│   └── headless/
├── logs/
│   ├── actions.jsonl
│   └── perf.jsonl        # only when JELLY_PERF_LOG is enabled
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
        └── registered/
```

Screenshot metadata records provenance, dimensions, trace linkage, and verification state. Screenshots capture Chromium-rendered page content, not the desktop. A specific target such as `css:body`, `css:main`, exact visible text, or a Jelly element ref can be captured directly.

Browser recordings support two modes. `continuous` streams renderer frames for the active Chromium target directly into FFmpeg, so individual frame files are not written to disk. `steps` captures a temporary browser screenshot after relevant successful actions, records redacted tool metadata, and builds a variable-frame-rate H.264 MP4 where each action frame is held for the configured duration. Step labels resolve human-readable target names before the action so the rendered text describes the action shown without exposing internal Jelly refs; labels use concise action-in-progress wording such as `clicking Submit`. Text-entry payloads are redacted from step metadata, and recorded URLs omit credentials, query strings, and fragments. Step videos use resolution-aware Jelly overlays: honey text, black backing, concise action labels, and a small favicon watermark. On stop, Jelly registers the MP4 and manifest, then removes all temporary recording files. The desktop is never part of either recording mode. `latest.png` remains a convenience path; reliability-sensitive workflows should pass artifact IDs.

Imported Chromium session data is copied into `profiles/headed/`. Profile data and `state/oauth.json` are sensitive local state and stay outside Git.

Cargo output lives separately in:

```text
/data/.jelly-build/
```

Clean runtime state:

```bash
scripts/clean-runtime.sh
```

Clean runtime state plus Cargo output:

```bash
scripts/clean-runtime.sh --build
```

Cleaning runtime state removes persisted OAuth clients/tokens, browser profiles, artifacts, routine continuations, test-run reports/locks, performance logs, and other local execution state.

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
