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
│   └── actions.jsonl
├── network/
│   └── requests.jsonl
├── routines/
├── injections/
└── artifacts/
    ├── metadata/
    │   └── artifact-*.json
    ├── screenshots/
    │   ├── latest.png
    │   └── registered/
    │       └── artifact-*.png
    └── downloads/
        └── registered/
```

Screenshot metadata records provenance, dimensions, trace linkage, and verification state. `latest.png` remains a convenience path; reliability-sensitive workflows should pass artifact IDs.

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

Cleaning runtime state removes persisted OAuth clients/tokens, browser profiles, artifacts, routine continuations, and other local execution state.

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
