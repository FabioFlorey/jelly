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
- Opt-in browser/CDP timing diagnostics via `config/jelly.toml` `[diagnostics].perf_log = true` and deterministic browser performance/regression fixtures.
- Canonical stable-ID scenario test suite with selectable groups/batches, per-run reports, concurrency locking, and a repository-level test index.
- Small-surface MCP Agent API with `browser-schema`, ordered semantic/raw `browser-call`, retained `browser-events`, logical target routing, and dedicated Agent API regression/benchmark coverage.
- Branded MCP operational index/dashboard/status surfaces with browser, recording, process, and OAuth state plus owner-gated inactive-process cleanup.
- Telegram HITL can attach a local MP4 instead of a screenshot and uses configurable title/subtitle/body/signature formatting with HTML-safe dynamic content.
- Paired OAuth setup can mint a one-time, five-minute owner-pairing link so installers can establish the owner browser without exposing the bootstrap secret to the browser.
- Interactive paired ChatGPT setup uses a loopback-only `/connect` page opened from an installer-authorized short-lived session (a bare GET is informational and cannot mint authorization), optionally opens a configured private Jelly plugin directly, redirects validated DCR/PKCE authorization through one-time `/approve` Yes/No controls, and returns the browser to ChatGPT; pairing remains the non-TTY/headless fallback.
- OAuth authorization-code clients receive rotating 30-day refresh tokens stored by hash; reuse of a consumed refresh token revokes its active token family.
- First-class semantic cookie and active-origin `localStorage`/`sessionStorage` operations, including HttpOnly cookie access through Chromium without requiring page JavaScript or raw CDP.
- First-class browser download lifecycle tracking with Chromium GUIDs, progress, suggested filenames, cancellation, terminal/failure state, artifact provenance, explicit destination materialization, and `fail`/`overwrite`/`uniquify` collision policy.

### Changed

- Jelly's public and local administration pages now share its logo, typography and honey-toned CSS, including styled OAuth/approval error pages. Routes are grouped by interface/operations/protocol, and `/connections` lists provider profiles without leaking credentials. Detailed `/dashboard` and `/status.json` (legacy `/status`) require owner or administrative access; `/health` and `/ready` remain public service probes. Added an isolated Chromium web UI screenshot smoke test.
- Active ChatGPT authorization detection now checks the registered ChatGPT OAuth client rather than assuming any stored refresh token belongs to ChatGPT; installer setup uses an authenticated loopback POST and does not create sessions through GET navigation.

- Screenshot capture now relies exclusively on Chromium/CDP. The Linux-specific `grim`/`hyprctl` desktop fallback was removed, screenshot base64 decoding now uses the Rust library instead of an external command, and element capture no longer scrolls the live page before taking a cropped screenshot.
- Step recordings now resolve human-readable target names before actions and use concise action-in-progress labels such as `filling Text input`, `selecting Two in Dropdown (select)`, and `clicking Submit`.
- Text-entry payloads are redacted from action traces and recording step metadata, and recorded URLs are sanitized to remove credentials, query strings, and fragments.
- Browser interactions now reject disabled controls and refuse to fill readonly inputs instead of mutating them through DOM setters.
- Exact-text target resolution now prefers matching interactive elements before generic containers, improving links and controls targeted by visible text.
- Visual highlights keep Jelly's `#ffc107` honey tone while `auto` now halos the principal rendered text of text-centric elements, uses a compact shape halo for controls, reuses fragment overlay nodes while following layout changes, and preserves the old rectangular treatment through explicit `box` mode.
- MCP browser primitives now reuse a persistent CDP session by default, configurable through `config/jelly.toml` `[mcp].persistent_session`. The cached session follows Jelly's shared active target, stale active-target state can recover through the primary page target, ambiguous failed side effects are not replayed, and dead sessions are discarded so a later call can reconnect.
- Interactive snapshots now use a mutation-invalidated in-page index with document-scoped non-DOM refs, optional positional pagination, bounded semantic search, and recursive open Shadow DOM traversal. Refs stay stable for the same connected element but change for replacement nodes, runtime reinstall, or navigation. `[page].runtime = false` selects the scanner/ref fallback path; runtime and legacy ref namespaces are isolated so stale optimized refs cannot fall through to DOM attributes.
- Runtime-backed target lookup prefers indexed interactive names before retaining the generic-text compatibility fallback. Interactive search and exact text targeting share deterministic ranking: semantic match quality, enabled before disabled, in-viewport before offscreen, then document order. Semantic naming handles associated labels, conservative adjacent labels for otherwise unlabeled controls, live value-derived names, ARIA disabled/checked state, and text projected through Shadow DOM slots.
- DOM-backed `@imgN` references returned by `inspect-images` now resolve through the normal target abstraction and can be reused by target-taking tools.
- MCP defaults to the `small-surface` browser mode through `config/jelly.toml`; `large-surface` exposes the expanded individual-tool surface. Raw CDP remains disabled unless `[mcp].raw_cdp` is enabled; when enabled, small-surface exposes it through `browser-call` and large-surface through a dedicated raw-only `cdp-call` tool.
- High-, mid-, and low-risk lifecycle boundaries were hardened after a function-level code review: profile replacement now stages and rolls back safely, recording start is serialized and publishes state atomically, OAuth consent/state handling fails explicitly, release MCP system tools no longer silently execute checkout code, navigation injection failures propagate, artifact metadata updates are atomic, auth mutex poisoning is recoverable, runtime cleanup failures are observable, stale browser PID reuse is rejected, and MCP initialization advertises Jelly's implemented protocol revision instead of echoing an arbitrary client value.
- Continuous recordings now follow Jelly's shared active browser target between frames and retain initial/final target provenance; routine graph system-tool failures now use the same typed classifications as MCP for profile import and browser recording.
- MCP service installation rebuild freshness now includes the compile-time embedded dashboard stylesheet and font assets.
- MCP installation and hosting now resolve the release build directory from the shared Cargo configuration, preventing installer/build-stamp drift from Cargo's configured target directory; an empty `JELLY_PUBLIC_URL` falls back to the local bind origin instead of becoming an empty OAuth issuer.
- Legacy `wait-download` now prefers matching event-backed lifecycle state and uses the historical filesystem completion heuristic only when no lifecycle record exists; graceful browser stop, detected launcher-process exit, restart, and tracker loss convert unfinished downloads to explicit interrupted state, and non-overwrite destination policies reserve paths atomically.
