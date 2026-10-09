<div id="top"></div>

# Development

**Audience:** Contributors and maintainers extending or testing Jelly.

## Design rules

- Browser primitives stay generic and composable.
- Shared behavior belongs in the Rust engine, not duplicated CLI binaries.
- JavaScript injection is an escape hatch, not the default implementation strategy.
- CLI tools, routines, and future agent runtimes consume the same primitive registry.
- Runtime state and build artifacts stay outside the repository.
- Browser test runs should leave no stray Chromium processes behind.

## Unified developer CLI

`./scripts/dev.sh --help` documents the single supported public command interface.
`./scripts/dev.sh doctor` is a read-only, build-free prerequisite audit; `setup`
replaces the former quickstart wizard and preserves hosting, OAuth, secret
backups, Telegram HITL, dry-run, and optional service installation.
`status` inspects installed services, and `start`/`stop` explicitly control them.
`build`, `format`, `lint`, `check`, `test` and `ci` reuse Cargo and Rust
maintenance tools. `test --isolated` runs the disposable MCP/Chromium suite;
`test --suite` **explicitly targets the installed Jelly service** and should
only run in a dedicated testing environment. `clean` previews its destructive
scope by default and requires `--yes` before invoking the existing
service-stop/runtime-removal implementation. No Python, Node.js or new shell
framework dependencies are required.

Branding is configured in trusted `config/dev.config.sh` and the canonical
`config/brand/full-logo.txt` asset. `scripts/lib/runtime.sh` and `ui.sh` expose
shared Bash helpers without compiling software, loading `.env` or modifying
services. `scripts/lib/config.sh` is also inert when sourced; commands that require
private deployment values must explicitly invoke `jelly_load_env`, which uses
the safe Rust data-only parser and preserves exported overrides.

## Tests

The test-system entry point is [`tests/INDEX.md`](../../tests/INDEX.md). The canonical behavioral suite lives in `tests/suite/`. Every test has a stable ID plus name, description, preconditions, input, expected output, and execution status. Every run receives a unique run ID and writes an auditable report under `/data/jelly-runtime/test-runs/<RUN_ID>/`.

Run the deterministic suite:

```bash
./scripts/dev.sh test --suite
# equivalent:
./scripts/dev.sh test --suite --batch deterministic
```

Run one test, one group, a named batch, or every enabled test:

```bash
./scripts/dev.sh test --suite --test SEM-005
./scripts/dev.sh test --suite --group highlight
./scripts/dev.sh test --suite --batch network
./scripts/dev.sh test --suite --all
```

Inspect the executable catalog and activation state:

```bash
./scripts/dev.sh test --suite --list
./scripts/dev.sh test --suite --catalog
./scripts/dev.sh test --suite --catalog --json
```

Tests/groups can be persistently disabled through `tests/suite/config/disabled-tests.txt` and `disabled-groups.txt`; `--include-disabled` overrides those files for an explicit diagnostic run. Batch definitions live in `tests/suite/config/batches.tsv`. See `tests/INDEX.md` for the group/batch map and `tests/suite/README.md` for the harness rationale, metadata contract, result schema, and selection rules.

The historical four-line `scripts/check-*.sh` forwarders have been removed. Use `./scripts/dev.sh test --suite --group NAME`, `--batch NAME`, or `--test ID` to select the same stable-ID tests. Unlike `test --isolated`, `test --suite` may interact with installed Jelly services.

Deterministic browser fixtures live in `tests/fixtures/` for delayed images, DOM rerenders, visibility, cookie/origin storage, download lifecycle/cancellation, performance, semantic targeting, highlighting, and Shadow DOM behavior. Network-dependent React/Selenium/Porsche cases are kept in the `network` batch because external deployments can change independently of Jelly and should not be treated as deterministic local regressions.

### Isolated Functional Core / Imperative Shell integration

The FC/IS migration can be exercised without using Jelly's installed systemd
browser service or touching `/data/jelly-runtime`:

```bash
./scripts/dev.sh test --isolated
# To retain the generated sandbox and its reports for troubleshooting:
JELLY_FCIS_KEEP_SANDBOX=true ./scripts/dev.sh test --isolated
```

The wrapper builds the **current working tree** (including untracked `src/core/`
and `src/shell/`) inside a freshly created `/data/jelly-fcis-isolated-*`
directory. It omits `.env`, rewrites **only the copied** `config/jelly.toml`
and Cargo target directory, and compiles separate test binaries. The Rust
harness `src/bin/jelly-fcis-probe.rs` refuses to run if its config,
`.env` or directory checks fail. Chromium uses disposable profiles and a
loopback-only CDP endpoint, and the MCP server binds a temporary loopback
port with synthetic credentials. The test driver terminates only the
subprocess groups it started; it does **not** invoke `systemctl`, Jelly's
browser launcher, the installed service, or any cleanup against the active
runtime. The wrapper removes its own sandbox afterward unless retention is
explicitly requested.

Integration checks cover Agent API batches and target/session behavior over
real CDP, ranking via the actual CLI, routine graph decisions and state
suspend/resume (the external HITL delivery adapter is **stubbed in the
sandbox**), download records/artifacts/collision policies on isolated disk,
OAuth code grants, refresh rotation/replay and persisted OAuth reload via
HTTP, plus cache invalidation and reconnect to a replacement Chromium without
restarting MCP. The on-disk Rust download test is marked `#[ignore]` and
**refuses I/O** unless its compiled runtime root matches the explicit
`JELLY_FCIS_ISOLATION_ROOT` sandbox opt-in. The wrapper additionally runs the
Rust/Chromium ranking fixture tests without Node.js.

These checks do **not** substitute for a real browser-originated CDP download
notification sequence, genuine third-party HITL delivery, OAuth persistence
fault injection, or a full deterministic Jelly suite against a separately
installed service. The canonical suite's browser helpers call the installed
`jelly-browser.service`, so do not run those helpers directly on a shared
active Jelly installation merely to validate this migration.

### Jelly web UI smoke test

For the shared human-facing Jelly design (home, connections, local activation,
OAuth approval), use the isolated HTTP + headless Chromium smoke test:

```bash
./scripts/dev.sh test --web-ui
# To keep render captures for visual review:
JELLY_UI_SCREENSHOTS_DIR=/tmp/jelly-ui-review ./scripts/dev.sh test --web-ui
```

This test starts a separate Jelly MCP process on temporary loopback ports and
checks public probes, protected status/dashboard responses, branded assets on
both public/admin listeners, and screenshots of `/connections` and `/connect`.
It never restarts the installed Jelly service or modifies `.env`. If the debug
`jelly-mcp` binary is missing, the smoke test builds it before starting the
isolated server. Screenshots are opt-in and the temporary browser
profile/processes are cleaned up. Rust unit tests additionally validate the
shared page template and local content-security policy (CSP).

## Agent instruction and tool-selection evaluation

The MCP `initialize` response includes [`.agent/instructions/mcp.md`](../../.agent/instructions/mcp.md). Keep it concise and normative; detailed scenarios live in the [agent playbook](../guides/AGENT_PLAYBOOK.md). Update tool descriptions and argument schemas in the Rust registries alongside behavioral instructions, then regenerate the tool index.

The [agent evaluation guide](./AGENT_EVALUATION.md) describes the offline routing/slot-filling scenarios and the separate browser-execution evidence needed for a valid overall success rate. Run `cargo run --locked --bin jelly-maint -- check agent-guidance --self-test` as a deterministic harness check; it does not call an LLM or measure real agent success.

## Script organization and service compatibility

`./scripts/dev.sh` is the single documented CLI. `scripts/commands/` contains
installation and service operations; `scripts/lib/` contains sourced Bash
configuration and UI helpers; `scripts/tests/` contains real isolated harnesses;
`scripts/diagnostics/` contains opt-in live-browser benchmarks; and
`scripts/maintenance/` contains the Rust support modules. The three top-level
`run-*.sh` scripts are intentional **systemd entrypoints**: installed unit files
reference `scripts/run-mcp-hosting.sh` and `scripts/run-cloudflare-tunnel.sh`,
so their paths must not be changed without a managed unit migration. The
`run-nip-io.sh` helper is invoked by the hosting launcher.

There are no separate Bash scripts for each semantic test group. The stable
IDs/groups and their runner live under `tests/suite/`. CLI changes require
`./scripts/dev.sh check`, which includes shell dispatch/security tests. Do
not create an additional wrapper for a command already exposed in `dev.sh`.

## Shell maintenance conventions

Jelly's shell entry points use Bash with `set -euo pipefail`; shared test functions are sourced by the strict suite runner. Keep paths and command arguments quoted, avoid executing `.env` as Bash, validate deletion targets **before** stopping services, and only terminate processes owned by the current invocation. Preserve active browser profiles and only remove router mappings created by the current launcher. Prefer temporary files and atomic replacement for persistent configuration or service files. Do not reintroduce forwarding `check-*.sh` scripts: the CLI dispatches directly into the canonical test suite.

After shell changes, run the offline safety checks and Bash parser across all tracked scripts:

```bash
cargo run --locked --bin jelly-maint -- check security
git ls-files '*.sh' | while IFS= read -r script; do bash -n "$script" || exit 1; done
```

The executable `QLT-019` test covers the data-only environment parser and deletion guards. ShellCheck and shfmt may be used separately when installed; they are not required by the Pages deployment.

## Project website and GitHub Pages

The static website is maintained in the top-level [`site/`](../../site) directory, separate from the Markdown documentation under `docs/` and the application's runtime branding assets under `assets/`. Website CSS and JavaScript live in `site/assets/css/style.css` and `site/assets/js/main.js`; media and favicon are under `site/assets/images/` and `site/favicon.svg`. The public site also serves standalone pages for Getting Started, Clients, MCP Tools, Examples, and Troubleshooting. Security details are maintained in `SECURITY.md`. The runtime server's `/connections`, `/dashboard` and OAuth approval interface is separate from the static GitHub Pages site.

[`.github/workflows/pages.yml`](../../.github/workflows/pages.yml) publishes the contents of `site/` directly using GitHub Actions on pushes to `main` affecting the site or publishing workflow, or via manual dispatch. The Pages workflow checks out the repository, packages the static site, and deploys it without website-validation steps. The separate [documentation workflow](../../.github/workflows/docs.yml) validates project Markdown and agent guidance, not the static website. To enable deployment, set **Settings → Pages → Build and deployment → Source → GitHub Actions** in the GitHub repository. The public project URL remains `https://fabioflorey.github.io/jelly/`. A `CNAME` file is deliberately absent because there is no configured custom domain; GitHub Pages ignores `CNAME` files when using the custom Actions deployment workflow. If adopting a custom domain, configure it in the repository's Pages settings and update the site URLs and `/jelly/`-based 404 assets accordingly.

## Documentation validation

Before submitting documentation changes, run:

```bash
cargo run --locked --bin jelly-maint -- check docs
```

The documentation checker verifies local links and anchors, the documentation index, fenced JSON/TOML examples, and basic Mermaid structure. The dedicated `.github/workflows/docs.yml` runs this check on documentation changes in CI. These checks validate syntax and navigation; they do **not** replace reviewing technical claims against the Rust implementation. Mermaid rendering in a browser is not part of this automated check.

## Generated tool documentation

Regenerate the tool index:

```bash
./scripts/dev.sh tools --generate
```

Verify it is current:

```bash
./scripts/dev.sh tools --check
```

CI performs the same freshness check. The [documentation validation](#documentation-validation) additionally checks Markdown links and machine-readable examples.

## Browser performance diagnostics

Browser performance changes are independently reversible while they are being evaluated:

- `[diagnostics].perf_log = true` writes opt-in CDP/connect timing events to the runtime log directory.
- `[mcp].surface = "large-surface"` selects the expanded MCP browser surface with individual browser primitives. The checked-in configuration selects `small-surface`; the required selector cannot be omitted from the TOML file. This mode replaces individual browser primitives with `browser-schema`, `browser-call`, and `browser-events` while retaining the exact same ordered system-tool set and bindings. System-tool aggregation/exposure changes are intentionally a separate migration.
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
./scripts/dev.sh benchmark browser --live
./scripts/dev.sh test --suite --batch deterministic
```

Browser profiling and surface benchmarking require explicit `--live` because they attach to the installed browser; they are not run by `dev.sh check` or offline CI. To target one historical subsystem, use `./scripts/dev.sh test --suite --group runtime`, `--group refs`, or any group listed by `--list`.

Performance conclusions should come from a purpose-built measurement for the change under review rather than a permanently maintained aggregate benchmark script.

For the isolated DOM-ranking policy seam (`src/core/ranking.js`), run
`cargo run --locked --bin jelly-maint -- test ranking`. The Rust harness uses
Chromium itself to evaluate the JavaScript policies and compare the current
runtime with the frozen pre-migration version 19 on semantic, Shadow DOM, and
large fixtures. It uses disposable browser profiles, no Jelly service, and
requires no Node.js. Timing benchmarks should be performed separately with
controlled loads; this harness tests correctness, not production latency.

### Interactive target resolution

When the page runtime is enabled, interactive elements receive document-scoped in-memory refs and pragmatic semantic names derived from element text plus relevant labeling attributes. A ref remains stable while the same connected DOM element survives rebuilds; replacement nodes receive new refs, and navigation or runtime reinstall creates a fresh ref namespace. Tokenized runtime refs resolve only through the in-memory runtime and never fall through to legacy `data-jelly-ref` attributes. Legacy numeric refs remain available only in rollback mode.

Semantic naming includes associated labels, a conservative immediate-sibling label heuristic for otherwise unlabeled form controls, live value-derived names, and text projected through `<slot>` elements in open Shadow DOM. `find-interactive` and exact interactive text targeting use deterministic priorities: semantic match quality, enabled before disabled, in-viewport before offscreen, then document order. If no interactive target matches, Jelly retains a generic non-interactive text fallback for compatibility. That fallback normalizes whitespace and uses a cheap text prefilter before the slower exact visibility/layout pass.

The runtime traverses normal DOM plus open Shadow DOM roots, including open roots attached after runtime installation. Nested slotted controls are resolved through their actual inner interactive element, while closed shadow roots remain opaque to normal Jelly DOM traversal. `[page].runtime = false` disposes the page runtime when the legacy snapshot/search path is used and restores DOM-backed refs. Re-enabling the runtime removes legacy refs and creates a fresh runtime namespace.

Limit/offset pagination is positional rather than snapshot-isolated. If structural mutations occur between page requests, offsets can shift; restart at offset 0 after such changes. Stable pagination across mutations would require a future cursor/snapshot token. Invalid or negative limits/offsets are rejected rather than silently expanding the result set.

## Runtime cleanup

> [!WARNING]
> Both variants of the cleanup script delete the configured runtime root **and** the Cargo build directory; `--build` also runs `cargo clean`. This permanently removes stored OAuth grants, browser profiles, logs, artifacts, custom extensions/userscripts, and compiled binaries. Back up data before running either command.

```bash
./scripts/dev.sh clean                 # Preview only
./scripts/dev.sh clean --yes           # Confirmed destructive cleanup
./scripts/dev.sh clean --build --yes   # Additionally run cargo clean
```

See the full [destructive cleanup contract](../reference/RUNTIME.md#destructive-cleanup).

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
