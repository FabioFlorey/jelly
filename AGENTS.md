<div id="top"></div>

# Jelly agent/developer instructions

## Table of Contents

- [Test governance — mandatory](#test-governance-mandatory)

## Test governance — mandatory

Before writing any new test (Rust, Bash, browser, MCP, mock, or E2E), read [tests/TAXONOMY.md](tests/TAXONOMY.md), section **Governance: decide before creating tests**. This is the authoritative policy; do not invent another test framework or runner.

1. Describe the **observable contract**, regression risk and boundary being verified.
2. Search existing shell scenarios (`./scripts/dev.sh test --catalog --json`), Rust inline tests (`rg -n 'cfg\(test\)|#\[test\]' src`), and `scripts/tests/`/isolated integration probes. Inspect expected assertions, not just similar test names.
3. Prefer **extending or parameterizing** an existing test. New IDs require a distinct contract or independent failure boundary, explained in review.
4. Existing runner: `tests/suite/run.sh`. Existing metadata: `tests/suite/config/classification.tsv`. Existing acceptance mapping: `tests/acceptance/`. **Do not add a new `.sh` test harness or mirrored registry** without a documented, reviewed need.
5. Keep `jt_register` ID stable; new classification rows start `provisional/active` and have an explicit triage rationale. Do not mark a test `retired` before confirming that its contract is no longer supported or its coverage is truly replaced.
6. Verify isolation and cleanup before any browser test. Shell `--suite` operates on installed browser state; prefer the isolated harness on shared development machines. Mocked engine-policy checks are never evidence of real sandbox activation, Docker/Podman interoperability, or remote-CDP isolation.
7. Quality gates, test cases, suites, acceptance decisions and execution evidence are **different models**. Don't claim UAT approval from an automated PASS or coverage from a proposed link.
8. Include in any testing change summary: previous tests searched, reason to reuse/add/consolidate, new or updated metadata, environmental dependencies, actual checks run, and gaps not executed.

**Existing CI:** `./scripts/dev.sh check` and `./scripts/dev.sh test`. Run targeted checks first. The `quality` test group is a legacy explicit gate, not part of default behavioral batches.

Other project-specific instructions: [docs/development/DEVELOPMENT.md](docs/development/DEVELOPMENT.md), [CONTRIBUTING.md](CONTRIBUTING.md).

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
