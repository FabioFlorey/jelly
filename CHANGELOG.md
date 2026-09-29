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
- Visual `highlight` and `clear-highlight` browser primitives for drawing a Jelly-owned, non-interactive overlay around a target in screenshots and recordings.

### Changed

- Screenshot capture now relies exclusively on Chromium/CDP. The Linux-specific `grim`/`hyprctl` desktop fallback was removed, screenshot base64 decoding now uses the Rust library instead of an external command, and element capture no longer scrolls the live page before taking a cropped screenshot.
- Step recordings now resolve human-readable target names before actions and use concise action-in-progress labels such as `filling Text input`, `selecting Two in Dropdown (select)`, and `clicking Submit`.
- Text-entry payloads are redacted from action traces and recording step metadata, and recorded URLs are sanitized to remove credentials, query strings, and fragments.
- Browser interactions now reject disabled controls and refuse to fill readonly inputs instead of mutating them through DOM setters.
- Exact-text target resolution now prefers matching interactive elements before generic containers, improving links and controls targeted by visible text.
- Visual highlights keep Jelly's `#ffc107` honey tone with a thinner border, softer glow, lighter fill, and smaller badge.
