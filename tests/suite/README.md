# Jelly test suite

For the test-system map and group index, start at [`../INDEX.md`](../INDEX.md).

This directory contains the canonical behavioral validation suite. It replaces ad-hoc pass/fail shell scripts with stable, selectable test cases that have explicit metadata and per-run records.

## Test model

Every registered test has the following fields:

- **ID** — stable identifier such as `SEM-005`.
- **Name** — short human-readable title.
- **Description** — what behavior is being validated and why.
- **Preconditions** — browser, fixture, credentials, network, or state required before execution.
- **Input** — the action or sequence supplied to Jelly.
- **Expected Output** — the observable state/result that constitutes success.
- **Status** — produced at execution time (`PASS`, `FAIL`, or `DISABLED`).

Every execution gets a **Test Run Unique ID**. Each result row records at minimum:

- Test Run Unique ID
- Test ID
- Status

The generated `report.tsv` joins those run fields with the complete test metadata, duration, and per-test log path.

## Layout

- `run.sh` — selection, execution, reporting, and run-ID generation.
- `lib.sh` — common assertions/browser helpers and test registration API.
- `groups/*.sh` — test implementations grouped by subsystem.
- `config/batches.tsv` — named multi-group batches.
- `config/disabled-tests.txt` — persistently disabled individual IDs.
- `config/disabled-groups.txt` — persistently disabled groups.
- `../fixtures/` — deterministic local browser fixtures.

Generated run artifacts are intentionally outside the repository at `/data/jelly-runtime/test-runs/<RUN_ID>/` by default.

## Groups and stable IDs

| Group | Prefix | What it validates |
| --- | --- | --- |
| `quality` | `QLT` | Rust/tooling quality gates plus test-harness metadata, `test-jelly` CLI contract, compact MCP instruction/discovery/raw-CDP documentation contracts, auxiliary Rust lockfile alignment, compact MCP surface budget, syntax, test-index coverage, and concurrency locking |
| `session` | `SES` | Persistent MCP/CDP reuse and browser/target recovery |
| `agent-api` | `API` | Dedicated compact/legacy Agent API regressions across catalog, discovery, browser-call, raw CDP, logical targets, events, stale refs, and failure policy |
| `events` | `EVT` | Idle CDP notification recovery, poll-barrier completeness, and explicit bounded-loss behavior |
| `runtime` | `RUN` | Page runtime indexing, invalidation, live state, ref stability, and rollback/reinstall |
| `refs` | `REF` | Document-scoped refs, DOM identity, stale refs, and namespace transitions |
| `semantic` | `SEM` | Accessible-name lookup, ranking, text fallback, visibility, and duplicate-text resolution |
| `shadow` | `SHD` | Open Shadow DOM discovery, interaction, geometry, artifacts, dynamic roots, and staleness |
| `ranking` | `RANK` | Limits, pagination, ranking order, and runtime/legacy parity |
| `highlight` | `HLT` | Highlight modes, geometry, follow behavior, cleanup, and validation |
| `rollback` | `RBK` | Persistent-session/page-runtime rollback matrices and transitions |
| `framework` | `FWK` | Network-dependent React mutation and reconciliation behavior |
| `real-world` | `WEB` | Network-dependent public-site browser regressions |

The executable catalog is the source of truth for individual cases. Use `tests/suite/run.sh --list` or `--catalog` rather than maintaining a second handwritten list of every test ID.

## Execution isolation

Executable suite runs are serialized with a global `flock` because Jelly owns shared browser/service state. Catalog/list operations do not need that lock. Each enabled scenario executes in its own `set -euo pipefail` subshell with stdout/stderr captured to its own log file.

Browser, network, and MCP selections trigger one binary-build preflight before scenario execution. Browser scenarios should arrange cleanup so failed tests do not leave Chromium or Jelly's user service behind.

## Running tests

Run one test:

```bash
tests/suite/run.sh --test SEM-005
```

Run several specific tests:

```bash
tests/suite/run.sh --test SEM-004 --test HLT-004
```

Run one group:

```bash
tests/suite/run.sh --group semantic
```

Run several groups:

```bash
tests/suite/run.sh --group runtime --group highlight
```

Run a named batch:

```bash
tests/suite/run.sh --batch deterministic
tests/suite/run.sh --batch network
```

Run every enabled test, including network-dependent groups:

```bash
tests/suite/run.sh --all
```

With no selector, the deterministic batch is used. Network-dependent tests are separated into the `network` batch because external sites can change independently of Jelly.

## Unified Cargo test frontend

Jelly provides a Cargo alias for a compact tabular Rust-test view:

```bash
cargo test-jelly
cargo test-jelly --all-targets
cargo test-jelly --lib error::tests
cargo test-jelly --all-targets -- --nocapture
```

All normal arguments are forwarded to the real `cargo test`; arguments after `--` remain libtest arguments. The wrapper preserves Cargo's exit code exactly. Use `--raw` when the captured Cargo/libtest output should also be printed.

Help is built in:

```bash
cargo test-jelly -h
cargo test-jelly --help
```

The terminal presentation follows Jelly's Quickstart theme: the shared full logo, honey `#ffc107` brand accents, green success state, red failure state, dim secondary text, and the same ordinary Unicode icon family. Colors are emitted only on an interactive terminal and honor `NO_COLOR`; `JELLY_NO_ICONS=true` replaces decorative Unicode icons with ASCII equivalents.

The same command exposes the canonical behavioral-test metadata without executing scenarios:

```bash
cargo test-jelly --catalog
cargo test-jelly --catalog --group semantic
cargo test-jelly --catalog --id QLT-003 --full
cargo test-jelly --catalog --json
```

The default catalog table includes ID, group, kind, enabled state, name, description, and preconditions. `--full` also includes input and expected output. Catalog rows come directly from the same `jt_register` records consumed by `tests/suite/run.sh`; `test-jelly` does not maintain a second metadata source. `--json` is intentionally undecorated: no logo, ANSI color, or prose is emitted, so it remains safe for scripts and `jq`.

## Listing and catalog metadata

List IDs and activation state:

```bash
tests/suite/run.sh --list
```

Print the complete metadata catalog as TSV:

```bash
tests/suite/run.sh --catalog
```

Or JSON:

```bash
tests/suite/run.sh --catalog --json
```

This catalog is generated from the same registrations that execute the tests, so documentation and executable IDs cannot silently drift apart.

## Activating and deactivating tests

Activation can be managed explicitly:

```bash
tests/suite/manage.sh disable-test SEM-005
tests/suite/manage.sh enable-test SEM-005
tests/suite/manage.sh disable-group real-world
tests/suite/manage.sh enable-group real-world
tests/suite/manage.sh status
```

The management command updates `config/disabled-tests.txt` and `config/disabled-groups.txt`. Those files can also be edited directly: one stable test ID or group name per line. Comments beginning with `#` and blank lines are ignored. A deliberately disabled test selected by a batch or `--all` is reported as `DISABLED` rather than silently disappearing.

For a one-off diagnostic run of disabled tests, use:

```bash
tests/suite/run.sh --include-disabled --test TEST-ID
tests/suite/run.sh --include-disabled --all
```

Named group batches are defined in `config/batches.tsv` as tab-separated `batch<TAB>group1,group2,...` entries. This lets CI, a developer, or an agent define stable test sets without copying command lists into other scripts.

## Result artifacts

A run directory contains:

- `run.json` — run identity, start time, branch and commit.
- `catalog.tsv` / `catalog.json` — complete catalog snapshot at run time.
- `results.jsonl` — one compact record per selected test containing Test Run Unique ID, Test ID, Status, duration, and log path.
- `report.tsv` — human/audit-friendly metadata + result table.
- `summary.json` — aggregate PASS/FAIL/DISABLED counts and result records.
- `logs/<TEST-ID>.log` — stdout/stderr for each case.
- `preflight-build.log` — browser/MCP binary build output when applicable.

The runner exits non-zero when any selected enabled test fails.

## Test design rationale

Tests are scenario-sized rather than assertion-sized. A stable ID describes one independently executable behavior with all of the setup necessary to reproduce it. A scenario may contain several internal assertions when they are inseparable parts of the same contract—for example, stale-target recovery checks the first failure, the reconnect, and repaired active-target state together.

Local fixtures are preferred for deterministic correctness. Live-site tests exist only where a real framework/site adds value that a fixture cannot. Their failures should be interpreted with the saved log and current site state because external deployment changes are not automatically Jelly regressions.

A test should assert observable behavior, not implementation trivia, unless the implementation detail is itself a compatibility/performance contract (for example, no `data-jelly-ref` mutation in optimized runtime mode or one persistent CDP connection).
