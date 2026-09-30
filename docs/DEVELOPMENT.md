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

The test-system entry point is [`tests/INDEX.md`](../tests/INDEX.md). The canonical behavioral suite lives in `tests/suite/`. Every test has a stable ID plus name, description, preconditions, input, expected output, and execution status. Every run receives a unique run ID and writes an auditable report under `/data/jelly-runtime/test-runs/<RUN_ID>/`.

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

- `JELLY_PERF_LOG=1` writes opt-in CDP/connect timing events to the runtime log directory.
- `JELLY_MCP_PERSISTENT_SESSION=0` restores one CDP connection/attach per MCP browser primitive.
- `JELLY_PAGE_RUNTIME=0` restores the legacy `snapshot-interactive` DOM scan and DOM-backed `data-jelly-ref` references.
- `JELLY_SNAPSHOT_LIMIT=<n>` bounds interactive snapshots by default in both runtime and legacy rollback paths. `0` keeps the compatibility behavior of returning all visible interactive elements.

These switches are diagnostic rollback paths, not separate supported execution modes. Keep the legacy paths behaviorally tested until the optimized paths have enough browser coverage to remove them deliberately.

The no-argument `snapshot-interactive` call intentionally remains unlimited for compatibility. A bounded default would silently hide targets because the current array result does not carry a truncation/cursor envelope. Prefer `snapshot-interactive <limit> [offset]` or `find-interactive <query> [limit] [offset]` when an agent does not need the entire interactive surface.

### Performance benchmark and regression checks

Use the local deterministic suite plus benchmark/profile commands before drawing conclusions from browser-performance changes:

```bash
scripts/benchmark-browser-perf.sh
scripts/profile-browser-runtime.sh
tests/suite/run.sh --batch deterministic
```

The subsystem compatibility wrappers remain available when a focused historical command is convenient (`scripts/check-page-runtime.sh`, `scripts/check-ref-semantics.sh`, and the other `scripts/check-*.sh` entries), but they delegate to the stable-ID suite.

The benchmark compares optimized and rollback modes on small, medium, large, mutation-heavy, and open-Shadow-DOM shapes. It records wall-clock latency, output bytes, CDP connection time, and `Runtime.evaluate` time. Measurements are machine-dependent and should be treated as comparative rather than universal performance claims.

September 2026 local benchmark runs show the shape of the tradeoff rather than a universal speed claim. Persistent MCP reuse reduced repeated large-page bounded snapshot latency by several milliseconds by removing per-call CDP attachment. Indexed semantic lookup and direct indexed text targeting were around 5 ms p50 on the large fixture, while the generic compatibility text scan remained much slower. Runtime `snapshot-interactive 50` was about 7 ms p50 / 14–15 KB, while the full runtime snapshot was roughly 70–80 ms and more than 400 KB. After moving legacy truncation into the page, legacy `snapshot-interactive 50` improved from roughly 60 ms to about 15 ms p50 / 11.5 KB while preserving legacy ref numbering. Full snapshots are not universally faster in runtime mode.

Fresh browser-side profiling on 1,501 interactive elements measured roughly 14–15 ms for a complete runtime rebuild, about 5–6 ms to describe the full result set, around 2 ms for all bounding-rect reads, around 1 ms for computed display/visibility reads, and around 1 ms for pure JSON serialization of the completed full result. A 22-open-root Shadow DOM fixture rebuilt in about 2 ms. These measurements do not justify caching geometry/visibility or maintaining a more complex incremental semantic-name database; explicit source-side limits remain the demonstrated high-value optimization.

### Interactive target resolution

When the page runtime is enabled, interactive elements receive document-scoped in-memory refs and pragmatic semantic names derived from element text plus relevant labeling attributes. A ref remains stable while the same connected DOM element survives rebuilds; replacement nodes receive new refs, and navigation or runtime reinstall creates a fresh ref namespace. Tokenized runtime refs resolve only through the in-memory runtime and never fall through to legacy `data-jelly-ref` attributes. Legacy numeric refs remain available only in rollback mode.

Semantic naming includes associated labels, a conservative immediate-sibling label heuristic for otherwise unlabeled form controls, live value-derived names, and text projected through `<slot>` elements in open Shadow DOM. `find-interactive` and exact interactive text targeting use deterministic priorities: semantic match quality, enabled before disabled, in-viewport before offscreen, then document order. If no interactive target matches, Jelly retains a generic non-interactive text fallback for compatibility. That fallback normalizes whitespace and uses a cheap text prefilter before the slower exact visibility/layout pass.

The runtime traverses normal DOM plus open Shadow DOM roots, including open roots attached after runtime installation. Nested slotted controls are resolved through their actual inner interactive element, while closed shadow roots remain opaque to normal Jelly DOM traversal. `JELLY_PAGE_RUNTIME=0` disposes the page runtime when the legacy snapshot/search path is used and restores DOM-backed refs. Re-enabling the runtime removes legacy refs and creates a fresh runtime namespace.

Limit/offset pagination is positional rather than snapshot-isolated. If structural mutations occur between page requests, offsets can shift; restart at offset 0 after such changes. Stable pagination across mutations would require a future cursor/snapshot token. Invalid or negative limits/offsets are rejected rather than silently expanding the result set.

## Runtime cleanup

```bash
scripts/clean-runtime.sh
scripts/clean-runtime.sh --build
```

<p align="right"><sub><a href="./README.md">⭐ Documentation index</a></sub></p>
