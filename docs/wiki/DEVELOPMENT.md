<div id="top"></div>

# Development

## Design rules

- Browser primitives stay generic and composable.
- Shared behavior belongs in the Rust engine, not duplicated CLI binaries.
- JavaScript injection is an escape hatch, not the default implementation strategy.
- CLI tools, routines, and future agent runtimes consume the same primitive registry.
- Runtime state and build artifacts stay outside the repository.
- Browser test runs should leave no stray Chromium processes behind.

## Tests

The test-system entry point is [`tests/INDEX.md`](../../tests/INDEX.md). The canonical behavioral suite lives in `tests/suite/`. Every test has a stable ID plus name, description, preconditions, input, expected output, and execution status. Every run receives a unique run ID and writes an auditable report under `/data/jelly-runtime/test-runs/<RUN_ID>/`.

Run the deterministic suite:

```bash
tests/suite/run.sh
# equivalent:
tests/suite/run.sh --batch deterministic
```

Run one test, one group, a named batch, or every enabled test:

```bash
tests/suite/run.sh --test SEM-005
tests/suite/run.sh --group highlight
tests/suite/run.sh --batch network
tests/suite/run.sh --all
```

Inspect the executable catalog and activation state:

```bash
tests/suite/run.sh --list
tests/suite/run.sh --catalog
tests/suite/run.sh --catalog --json
```

Tests/groups can be persistently disabled through `tests/suite/config/disabled-tests.txt` and `disabled-groups.txt`; `--include-disabled` overrides those files for an explicit diagnostic run. Batch definitions live in `tests/suite/config/batches.tsv`. See `tests/INDEX.md` for the group/batch map and `tests/suite/README.md` for the harness rationale, metadata contract, result schema, and selection rules.

The legacy `scripts/check-*.sh` commands remain as compatibility wrappers into the same suite, so there is only one executable source of truth.

Deterministic browser fixtures live in `tests/fixtures/` for delayed images, DOM rerenders, visibility, downloads, performance, semantic targeting, highlighting, and Shadow DOM behavior. Network-dependent React/Selenium/Porsche cases are kept in the `network` batch because external deployments can change independently of Jelly and should not be treated as deterministic local regressions.

## Generated tool documentation

Regenerate the tool index:

```bash
scripts/build-tool-index.sh
```

Verify it is current:

```bash
scripts/check-tool-index.sh
```

CI performs the same freshness check.

## Browser performance diagnostics

Browser performance changes are independently reversible while they are being evaluated:

- `[diagnostics].perf_log = true` writes opt-in CDP/connect timing events to the runtime log directory.
- `[mcp].surface = "large-surface"` selects the expanded MCP browser surface with individual browser primitives. When the selector is absent, MCP defaults to `small-surface`, which replaces individual browser primitives with `browser-schema`, `browser-call`, and `browser-events` while retaining the exact same ordered system-tool set and bindings. System-tool aggregation/exposure changes are intentionally a separate migration.
- `[mcp].persistent_session = false` restores one CDP connection/attach per MCP browser primitive/builtin.
- `[page].runtime = false` restores the legacy `snapshot-interactive` DOM scan and DOM-backed `data-jelly-ref` references.
- `[page].snapshot_limit = <n>` bounds interactive snapshots by default in both runtime and legacy rollback paths. `0` keeps the compatibility behavior of returning all visible interactive elements.

These switches are diagnostic rollback paths, not separate supported execution modes. `large-surface` and `small-surface` are the only supported MCP surface names. Other legacy page-runtime/performance rollback paths remain behaviorally tested until their own removal criteria are established.

Logical browser target labels are runtime state shared by MCP and CLI processes. The mapping is persisted atomically in `/data/jelly-runtime/state/logical_targets.json` under an OS file lock, reconciled from live `Target.getTargets` data, and reset with the browser lifecycle. Temp files use a process/nonce identity, are synced before rename, and the state directory is synced after replacement. Only logical-label/target-ID state is persisted; attached CDP session IDs remain in-memory because they are ephemeral connection state.

Each `BrowserSession` also owns a bounded in-memory CDP notification ring and its runtime-scoped event subscriptions. The default limits are 1,024 retained notifications and 4 MiB of incoming notification bytes. Every observed notification consumes a monotonic sequence number; count/byte eviction increments `dropped`, the latest dropped sequence is tracked for precise cursor-loss detection, and stream resets are counted separately. `browser-events poll` drains pending websocket traffic with a browser-scoped `Target.getTargets` barrier before applying target/method filters. The receive loop remains synchronous: no background reader drains events while the browser session is idle.

This synchronous design was retained deliberately after idle-event measurements. A real Chromium probe scheduled 64 target-scoped Runtime console notifications after the scheduling request returned, performed no websocket reads during the idle period, and recovered all 64 in order on the next poll with no drop/reset/cursor loss. A 1,500-event idle burst also drained successfully; the 1,024-entry ring retained the newest 1,024 events and reported exactly 476 bounded evictions with `cursor_lost=true`. These measurements support the current poll-based contract without introducing a concurrent reader. They do not promise continuous low-latency delivery while Jelly is idle; a future requirement for that behavior, or evidence of loss before ring admission, would justify revisiting the driver architecture.

BrowserSession enables Target discovery plus flattened auto-attach filtered to `page` targets. Each live page therefore has an in-memory CDP session binding alongside its persisted logical identity (`main`, `tab-N`). Session IDs are never persisted or exposed through the Agent API. Target-scoped raw CDP addressed to a logical target uses that target's attached session directly and does not activate/switch the page. `Target.targetInfoChanged` preserves the session across navigation metadata churn; detach/crash invalidates the session binding; destroy removes the logical target while the retained destroy event keeps its pre-removal logical label.

The no-argument `snapshot-interactive` call intentionally remains unlimited for compatibility. A bounded default would silently hide targets because the current array result does not carry a truncation/cursor envelope. Prefer `snapshot-interactive <limit> [offset]` or `find-interactive <query> [limit] [offset]` when an agent does not need the entire interactive surface.

### Performance profiling and regression checks

Use the deterministic suite plus the focused runtime profiler when investigating browser-performance changes:

```bash
scripts/profile-browser-runtime.sh
tests/suite/run.sh --batch deterministic
```

The subsystem compatibility wrappers remain available when a focused historical command is convenient (`scripts/check-page-runtime.sh`, `scripts/check-ref-semantics.sh`, and the other `scripts/check-*.sh` entries), but they delegate to the stable-ID suite.

Performance conclusions should come from a purpose-built measurement for the change under review rather than a permanently maintained aggregate benchmark script.

### Interactive target resolution

When the page runtime is enabled, interactive elements receive document-scoped in-memory refs and pragmatic semantic names derived from element text plus relevant labeling attributes. A ref remains stable while the same connected DOM element survives rebuilds; replacement nodes receive new refs, and navigation or runtime reinstall creates a fresh ref namespace. Tokenized runtime refs resolve only through the in-memory runtime and never fall through to legacy `data-jelly-ref` attributes. Legacy numeric refs remain available only in rollback mode.

Semantic naming includes associated labels, a conservative immediate-sibling label heuristic for otherwise unlabeled form controls, live value-derived names, and text projected through `<slot>` elements in open Shadow DOM. `find-interactive` and exact interactive text targeting use deterministic priorities: semantic match quality, enabled before disabled, in-viewport before offscreen, then document order. If no interactive target matches, Jelly retains a generic non-interactive text fallback for compatibility. That fallback normalizes whitespace and uses a cheap text prefilter before the slower exact visibility/layout pass.

The runtime traverses normal DOM plus open Shadow DOM roots, including open roots attached after runtime installation. Nested slotted controls are resolved through their actual inner interactive element, while closed shadow roots remain opaque to normal Jelly DOM traversal. `[page].runtime = false` disposes the page runtime when the legacy snapshot/search path is used and restores DOM-backed refs. Re-enabling the runtime removes legacy refs and creates a fresh runtime namespace.

Limit/offset pagination is positional rather than snapshot-isolated. If structural mutations occur between page requests, offsets can shift; restart at offset 0 after such changes. Stable pagination across mutations would require a future cursor/snapshot token. Invalid or negative limits/offsets are rejected rather than silently expanding the result set.

## Runtime cleanup

```bash
scripts/clean-runtime.sh
scripts/clean-runtime.sh --build
```

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
