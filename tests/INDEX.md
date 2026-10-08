# Jelly tests

This directory is the entry point for Jelly validation.

Jelly uses two complementary test layers:

- Rust tests, run with `cargo test-jelly --all-targets` for the tabular frontend (or directly with `cargo test --all-targets`), for unit- and target-level behavior.
- The scenario suite in [`suite/`](./suite/README.md), which exercises Jelly through its binaries, MCP boundary, browser runtime, fixtures, rollback modes, and real-world browser workflows.

## Start here

For Rust validation with a compact per-target table:

```bash
cargo test-jelly --all-targets
```

For normal behavioral repository validation:

```bash
tests/suite/run.sh
```

That runs the `deterministic` batch. To inspect the available executable cases before running them, use either the raw suite commands or the tabular Cargo frontend:

```bash
cargo test-jelly --catalog
cargo test-jelly --catalog --id QLT-003 --full

tests/suite/run.sh --list
tests/suite/run.sh --catalog
tests/suite/run.sh --catalog --json
```

The detailed suite contract, command reference, reporting format, and test-design rationale are documented in [`suite/README.md`](./suite/README.md).

## Directory map

| Path | Purpose |
| --- | --- |
| [`suite/run.sh`](./suite/run.sh) | Canonical scenario-test runner: selection, locking, preflight builds, execution, and reporting |
| [`suite/lib.sh`](./suite/lib.sh) | Test registration, assertions, browser lifecycle helpers, fixture helpers, and common utilities |
| [`suite/groups/`](./suite/groups/) | Scenario implementations grouped by subsystem |
| [`suite/config/`](./suite/config/) | Named batches plus persistent test/group disable lists |
| [`fixtures/`](./fixtures/) | Deterministic local pages used by browser tests |
| [`suite/README.md`](./suite/README.md) | Full suite documentation |

Generated test-run data does not belong in this directory. By default it is written to:

```text
<runtime_root>/test-runs/<RUN_ID>/
```

Browser/build runtime state likewise stays outside the repository.

## Agent-routing evaluation

[`fixtures/agent-guidance-cases.json`](./fixtures/agent-guidance-cases.json) contains deterministic intent-to-tool cases for the [offline evaluator](../scripts/eval-agent-guidance.py). Run `python3 scripts/eval-agent-guidance.py --self-test` to check its scoring contract without starting a browser. Real agent decisions can be evaluated with `--predictions`, but verified end-to-end browser task success requires a separate execution run. See [Agent Guidance Evaluation](../docs/development/AGENT_EVALUATION.md).

## Test groups

| Group | Scope |
| --- | --- |
| `quality` | Formatting, compilation, Rust tests, Clippy, generated documentation, suite metadata/syntax, shell environment/cleanup safety, `test-jelly` CLI contract, small-surface MCP instruction/discovery/raw-CDP documentation contracts, auxiliary Rust lockfile alignment, small-surface MCP surface budget, test-index coverage, and suite locking |
| `session` | Persistent MCP/CDP session reuse, active-target synchronization, and reconnect behavior |
| `agent-api` | Small-surface/large-surface Agent API catalog, discovery, browser-call, raw-CDP, logical-target, event, stale-ref, and failure-policy regressions |
| `events` | Idle CDP notification recovery, synchronous poll-barrier completeness, and bounded-loss accounting |
| `runtime` | In-page runtime indexing, mutation invalidation, live state, ref stability, and runtime reinstall/rollback |
| `refs` | Document-scoped refs, DOM identity, stale-ref collision resistance, and namespace transitions |
| `semantic` | Accessible-name extraction, semantic lookup/ranking, generic text fallback, visibility, and text targeting |
| `shadow` | Open Shadow DOM discovery, interaction, geometry, dynamic roots, and stale descendants |
| `ranking` | Limits, pagination, ordering, and runtime/legacy ranking parity |
| `highlight` | Highlight modes, geometry, target-following behavior, cleanup, and validation |
| `rollback` | Compatibility matrices for persistent sessions, page runtime, ref namespaces, and snapshot limits |
| `browser-state` | First-class cookie/DOM-storage semantics and CDP-backed download lifecycle, cancellation, destination, and restart/stop behavior |
| `framework` | Network-dependent React mutation/reconciliation scenarios |
| `real-world` | Network-dependent public-site regression scenarios |

## Named batches

The authoritative definitions live in [`suite/config/batches.tsv`](./suite/config/batches.tsv).

| Batch | Intended use |
| --- | --- |
| `deterministic` | Default local validation; includes quality and deterministic browser/MCP groups |
| `browser` | Deterministic browser/MCP behavior without the quality group |
| `fast` | Smaller focused subset for quick iteration |
| `network` | External React and real-world website tests; failures can reflect upstream site changes |

Use stable test IDs when isolating a failure:

```bash
tests/suite/run.sh --test SEM-005
```

Use a group when working on one subsystem:

```bash
tests/suite/run.sh --group runtime
```

Use `--all` only when network-dependent cases are also appropriate:

```bash
tests/suite/run.sh --all
```

## Execution model

Each group file registers scenarios through `jt_register`. A registration contains a stable ID, group, name, description, preconditions, input, expected output, kind, and Bash function. The runner builds its catalog from those same registrations, so the executable catalog and test metadata share one source of truth.

Executable suite runs are serialized with a global `flock` because Jelly owns shared browser/service state. Individual scenarios run in subshells with strict Bash settings and receive their own logs. Browser/MCP scenarios use the current Jelly binaries after a shared preflight build.

A selected disabled test is reported as `DISABLED` rather than silently omitted. See [`suite/README.md`](./suite/README.md) for enable/disable commands and the complete result-artifact schema.

## Validation expectation

A test implementation is not considered trustworthy merely because its shell function exists. Repository validation should cover the harness itself as well as the product behavior. The `quality` group therefore checks Rust formatting/build/tests/Clippy, generated-tool-index consistency, suite catalog integrity, shell syntax, test-index coverage, and the global suite lock.

For project-level development guidance, see [`../docs/development/DEVELOPMENT.md`](../docs/development/DEVELOPMENT.md).
