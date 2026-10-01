# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Initial repository structure.
- Base GitHub community files.
- Agent workspace directory.
- Stable MCP result envelopes and typed reliability errors.
- Explicit verification primitives and bounded condition waits.
- Provenance-bearing screenshot artifacts with integrity/semantic verification metadata.
- Guarded JSON routine graphs with branching, cycles, execution budgets, HITL continuation, and owned-browser cleanup.
- Chromium profile import for session reuse.
- Visual `highlight` and `clear-highlight` browser primitives for drawing Jelly-owned, non-interactive target halos in screenshots and recordings. `highlight` supports `auto`, `content`, and legacy `box` geometry modes.
- `find-interactive` for bounded, source-ranked, pageable semantic searches over interactive browser targets.
- Optional limit/offset pagination for `snapshot-interactive`.
- Opt-in browser/CDP timing diagnostics via `JELLY_PERF_LOG=1` and deterministic browser performance/regression fixtures.
- Canonical stable-ID scenario test suite with selectable groups/batches, per-run reports, concurrency locking, and a repository-level test index.
- Compact MCP Agent API with `browser-schema`, ordered semantic/raw `browser-call`, retained `browser-events`, logical target routing, and dedicated Agent API regression/benchmark coverage.

### Changed

- Screenshot capture now relies exclusively on Chromium/CDP. The Linux-specific `grim`/`hyprctl` desktop fallback was removed, screenshot base64 decoding now uses the Rust library instead of an external command, and element capture no longer scrolls the live page before taking a cropped screenshot.
- Step recordings now resolve human-readable target names before actions and use concise action-in-progress labels such as `filling Text input`, `selecting Two in Dropdown (select)`, and `clicking Submit`.
- Text-entry payloads are redacted from action traces and recording step metadata, and recorded URLs are sanitized to remove credentials, query strings, and fragments.
- Browser interactions now reject disabled controls and refuse to fill readonly inputs instead of mutating them through DOM setters.
- Exact-text target resolution now prefers matching interactive elements before generic containers, improving links and controls targeted by visible text.
- Visual highlights keep Jelly's `#ffc107` honey tone while `auto` now halos the principal rendered text of text-centric elements, uses a compact shape halo for controls, reuses fragment overlay nodes while following layout changes, and preserves the old rectangular treatment through explicit `box` mode.
- MCP browser primitives now reuse a persistent CDP session by default, with `JELLY_MCP_PERSISTENT_SESSION=0` available as a rollback switch. The cached session follows Jelly's shared active target, stale active-target state can recover through the primary page target, ambiguous failed side effects are not replayed, and dead sessions are discarded so a later call can reconnect.
- Interactive snapshots now use a mutation-invalidated in-page index with document-scoped non-DOM refs, optional positional pagination, bounded semantic search, and recursive open Shadow DOM traversal. Refs stay stable for the same connected element but change for replacement nodes, runtime reinstall, or navigation. `JELLY_PAGE_RUNTIME=0` restores the legacy scanner/ref path; runtime and legacy ref namespaces are isolated so stale optimized refs cannot fall through to DOM attributes.
- Runtime-backed target lookup prefers indexed interactive names before retaining the generic-text compatibility fallback. Interactive search and exact text targeting share deterministic ranking: semantic match quality, enabled before disabled, in-viewport before offscreen, then document order. Semantic naming handles associated labels, conservative adjacent labels for otherwise unlabeled controls, live value-derived names, ARIA disabled/checked state, and text projected through Shadow DOM slots.
- DOM-backed `@imgN` references returned by `inspect-images` now resolve through the normal target abstraction and can be reused by target-taking tools.
- MCP now defaults to the `small-surface` browser mode when `JELLY_MCP_SURFACE` is unset. `large-surface` exposes the expanded individual-tool surface. Raw CDP remains disabled unless independently enabled with `JELLY_MCP_RAW_CDP`; when enabled, small-surface exposes it through `browser-call` and large-surface through a dedicated raw-only `cdp-call` tool.
