<div id="top"></div>

# Test taxonomy, evidence and acceptance

## Table of Contents

- [Test catalog and its metadata](#test-catalog-and-its-metadata)
- [Governance: decide before creating tests](#governance-decide-before-creating-tests)
- [Test suites are selections, not classifications](#test-suites-are-selections-not-classifications)
- [Acceptance criteria are independently governed](#acceptance-criteria-are-independently-governed)
- [Runtime metadata and gaps](#runtime-metadata-and-gaps)

This document separates three independent concepts: **cataloged tests**, **test suites**, and **acceptance criteria**. Levels, resource sizes, focus areas and environments are different axes. **E2E, UAT, smoke and regression are not interchangeable test levels.**

## Test catalog and its metadata

`tests/suite/groups/*.sh` remains the canonical executable scenario registry (`jt_register`). Its stable ID is never changed just to reclassify a test. `tests/suite/config/classification.tsv` is the mandatory one-row-per-ID sidecar for orthogonal metadata and explicit maintenance triage. The runner validates its enums, detects unknown/duplicate IDs and refuses missing classifications. This avoids editing 91 existing executable registration signatures.

| Metadata | Available before | Added now | Meaning |
| --- | --- | --- | --- |
| `id` | Yes | No | Immutable scenario identifier, e.g. `API-009`. |
| `group` | Yes | No | Functional domain/subsystem, e.g. `agent-api`. |
| `kind` | Yes | No | **Legacy execution dependency**: `quality`, `browser`, `mcp`, `network`; not the testing level. |
| `name`, `description` | Yes | No | Human-readable intent. |
| `preconditions`, `input`, `expected_output` | Yes | No | Test specification, not measured results. |
| `enabled` | Yes | No | Whether the scenario is included unless disabled; not its last status. |
| `scope` | No | Yes | Exclusive test boundary: `unit`, `component`, `system`, `cross-system`, `not-applicable`, `unclassified`. |
| `size` | No | Yes | Resources: `small`, `medium`, `large`, `unclassified`. Independent of scope. |
| `focus` | No | Yes | Main assertion: `functional`, `non-functional`, `tooling`, `unclassified`. |
| `execution` | No | Yes | `automated`, `manual`, `hybrid`. |
| `environment` | Indirect via kind/preconditions | Yes | Primary required runtime: `local-toolchain`, `native-browser`, `installed-mcp`, `public-web`, `docker`, `podman`, `remote-cdp`, `unclassified`. |
| `review_status` | No | Yes | `provisional` (inferred mapping, needs individual review) or `reviewed`. |
| `lifecycle` | No | Yes | `active`, `deprecated`, `retired` supported-contract state. |
| `triage` / `related_ids` / `triage_reason` | No | Yes | Maintenance disposition, related test IDs and specific rationale; this is not a runtime result or a duplicate contract body. |

### Scope boundaries (mutually exclusive)

- `unit`: one logical unit without real process/external interfaces, usually an inline Rust test.
- `component`: one subsystem or module interface in isolation, optionally with mocks.
- `system`: an installed Jelly runtime exercised as a whole with controlled local dependencies (e.g. its Chromium fixture).
- `cross-system`: an interaction that depends on another independently deployed service or public website.
- `not-applicable`: checks that are quality gates rather than product-behavior tests, e.g. formatting and Clippy.
- `unclassified`: unknown; do not guess.

A test with complete input-to-output behavior may be called *end-to-end* in everyday language, but `E2E` is not an independent value of `scope` here. Tests can be both system-level and supporting evidence for a business acceptance criterion.

### Size and focus

`small`: constrained local logic without service/process/network dependencies; `medium`: local tools/processes and reproducible fixtures; `large`: external machines, networks or uncontrolled services. Size is **not** a synonym for the amount of source code covered.

`functional` checks behavior; `non-functional` checks quantitative or quality attributes (performance, reliability, resource consumption, security properties); `tooling` represents static quality gates or checks of the repository toolchain. A test can measure secondary concerns, but this field stores one *primary* intent.

**Migration status:** The 91 scenarios have provisional scope/size/focus metadata inherited from their groups and an **individual source-level review of their observable contracts and assertions** for maintenance triage. The original guessed triage was corrected after comparing actual implementation boundaries. **All 91 remain provisional for runtime verification**, and inline `#[cfg(test)]` Rust tests are not yet represented as individual records in this shell scenario catalog. Rust unit tests continue to run with `cargo test --lib` or `cargo test --all-targets`. Do not interpret absence of `scope=unit` from the shell catalog as absence of unit tests in Jelly.

### Querying

```bash
./scripts/dev.sh test --catalog --scope system --environment native-browser
./scripts/dev.sh test --catalog --scope cross-system --json
tests/suite/run.sh --catalog --focus functional --review provisional --json
tests/suite/run.sh --scope system --environment native-browser  # EXECUTES tests; may modify installed browser
cargo test-jelly --catalog --scope component --json
```

Filters on different axes intersect. The same filter may occur multiple times in the shell runner to select a union within that axis. Listing or reading catalog metadata never starts Chromium. **Executing** shell scenarios may access/alter an installed Jelly service; review the environment before invoking them.

## Governance: decide before creating tests

**Normative rule for developers and agents:** do not create a new test file, Bash harness, fixture, Rust test, or test runner before checking the existing coverage and explaining why it cannot be reused.

### Authoritative models (no mirrored definitions)

| Model | Authoritative source | Rule |
| --- | --- | --- |
| Test case / contract | `tests/suite/groups/*.sh` via `jt_register` (`id`, `preconditions`, `input`, `expected_output`); Rust-inline tests keep their Rust test name and assertion contract | One executable implementation per independently necessary behavior/boundary. Do not copy expectations into another registry. |
| Taxonomy and maintenance triage | `tests/suite/config/classification.tsv` | Exactly one row per shell scenario ID. Defines scope/size/focus/execution/environment/review status, lifecycle, triage, related IDs and rationale. It does not repeat the executable test body. |
| Test suite | `tests/suite/config/suites.tsv` | Sets of stable IDs, no duplicate implementation; groups and legacy batches are selectors, not new tests. |
| Acceptance requirement and approval | `tests/acceptance/criteria.tsv` | The acceptance decision belongs to its business owner; test success is not UAT approval. |
| Requirement-to-evidence map | `tests/acceptance/links.tsv` | Primary/supporting references to stable IDs, not another copy of a test. |
| Test execution / evidence | Runner `results.jsonl`, `run.json`, `report.tsv`, logs | Status reflects an actual invocation. Never treat a metadata link or mock PASS as a verified browser E2E. |

**Before adding or expanding a test, follow this order:**

1. State the change as an **observable contract**: input, boundary (which public API or subsystem), expected result, and regression risk. If business-facing, identify the acceptance criterion.
2. **Search all three current implementations**, not just `tests/suite/`: the scenario catalog, inline Rust `#[cfg(test)]` tests, and standalone harnesses under `scripts/tests/` plus `src/bin/jelly-fcis-probe.rs`. Also inspect the existing fixture and implementation. Example read-only searches:

   ```bash
   ./scripts/dev.sh test --catalog --json | jq -r '.[] | [.id,.group,.name,.expected_output,.triage,(.related_ids|join(","))] | @tsv'
   rg -n 'download|browser_id|sandbox' src tests scripts/tests
   ./scripts/dev.sh test --catalog --triage consolidate --json
   ```

3. **Modify an existing test** if it covers the same contract at the same boundary; prefer adding a fixture row, case, or environment parameter to a second near-identical executable.
4. **Keep an additional test** only if it proves a *distinct contract*, or the same contract at a demonstrably independent failure boundary (e.g. Rust parser versus CDP transport versus real Docker). Document exactly what extra failure it detects; similar names alone are not proof of duplication.
5. Create a new stable ID **only after** steps 1–4. Register its behavior once, add one classification row with `review_status=provisional`, `lifecycle=active`, `triage=retain`, and a rationale; update named suites or requirement links only if relevant. Never create a new `.sh` runner merely to exercise an existing interface.
6. Declare **environment, prerequisites, isolation, cleanup, and actual evidence**. A mocked Docker CLI validates argument forwarding, **not** kernel sandbox activation; a native browser test does **not** prove multi-browser remote isolation. Local deterministic fixtures are preferred; live sites belong to optional compatibility runs.
7. If replacing an old test, record the old/new IDs and the proof of equivalent coverage *before* retirement. Explicitly decide whether legacy behavior is supported; do not delete compatibility tests merely because their implementation is old.
8. Run only the smallest relevant existing checks during development, then `./scripts/dev.sh check`, appropriate Rust tests, and an isolated browser run if the changed contract requires it. Never start a destructive installed-browser suite on a shared machine without explicit awareness.

**Review gate for test PRs:** the change description must name the observable contract, previous coverage searched, *reuse vs new test* decision, execution boundary/environment, validation results, and any retired or consolidated IDs. A reviewer rejects new tests which only reassert another PASS without explaining distinct value.

### Lifecycle, triage, review and execution are different

- `lifecycle=active`: currently supported behavior. `deprecated`: still supported but a retirement decision/deadline is required. `retired`: no longer run, kept for traceability; requires reviewed classification. A test does not become obsolete because it is old.
- `triage=retain`: currently distinct; `consolidate`: investigate shared contract/setup/gate execution; `reassess`: inspect overlap or confirm the compatibility policy before a change. **Triage is not permission to delete.**
- `review_status=provisional` indicates that classifications and triage are **not validated** against live behavior. Set `reviewed` only after manual contract/implementation inspection and relevant execution evidence. Do not bulk-upgrade reviews.
- `PASS/FAIL/DISABLED` are run statuses. The runner does not yet distinguish environment `SKIPPED` from `NOT_RUN`; absence of E2E results must never be presented as success.
- Source-level audit of **91 cases**: **79 retain, 10 consolidate, 2 reassess**; all stay **active and provisional for actual runtime verification**. The 10 `consolidate` decisions concern duplicate *quality gate invocation*: several gates add auxiliary-crate coverage, which must survive a migration. No deletion, deprecation, or retirement has been authorized. `QLT-015` (fixed ratio budgets; primitive counts derive from the registry) and `WEB-003` (public-site volatility) remain for review of budget policy/third-party dependence, **not** because they duplicate local tests. The seven native MCP session/rollback tests (`SES-*`, `RBK-*`) had a stale public HTTP issuer assumption; the fixtures now use a synthetic HTTPS public issuer while calling the local HTTP listener. `QLT-013` was adjusted to the current per-browser session and operator-chosen raw CDP policy. These corrections pass read-only/static checks but do not constitute live browser execution.
- The inline Rust tests and standalone harnesses are separate from this scenario catalog. Do not claim the catalog covers all unit tests; harmonize inventory via existing tooling before adding another registry.

### Execution boundaries and duplicate work

`./scripts/dev.sh check` owns formatting, Clippy (including all auxiliary Rust crate manifests), docs/security/architecture validation and selected offline tooling checks. `./scripts/dev.sh test` is the **single canonical** `cargo test --all-targets --locked` execution, covering library **and** binary targets; CI must not repeat selected binary tests. `./scripts/dev.sh ci` is only `check` followed by `test`. `QLT-001`, `QLT-003` and `QLT-004` delegate to these canonical commands if an operator explicitly selects them; that is an alternative entry point, not a separate duplicate implementation. The legacy `quality` group remains callable explicitly for compatibility, but **`deterministic` and `fast` no longer embed it**, avoiding automatic repeated builds, lint and Cargo tests. The `network` batch is optional and never a deterministic CI gate. `test --isolated` runs a distinct disposable MCP/Chromium integration harness. Distinct gate names in different entry points do not imply distinct evidence.

The suite runner's selection contract is **union among --test, --suite, --group, --batch**, **intersection across classification dimensions**, and **union of repeated values within a dimension**. The catalog preview and execution use identical selection semantics; invalid selectors fail explicitly.

## Test suites are selections, not classifications

`tests/suite/config/suites.tsv` contains named, explicit sets of stable test IDs. Tests may belong to multiple suites without duplicates. Initial suites:

- `smoke-offline`: complementary offline checks (`API-001`, `API-006`, `QLT-007`, `QLT-009`, `QLT-013`) that are **not** the formatting/lint/catalog-policy checks already run by `dev.sh check`.
- `smoke-browser`: representative browser/MCP smoke checks **requiring the installed browser**.

```bash
tests/suite/run.sh --catalog --suite smoke-offline --json  # preview
tests/suite/run.sh --suite smoke-offline                   # executes
tests/suite/run.sh --suite smoke-browser                   # accesses installed Chromium
```

The existing `tests/suite/config/batches.tsv` is retained for backward compatibility as **group-based selection presets**, notably `deterministic` (all local browser + MCP session/rollback scenarios), `browser` (browser/API behavior without the installed MCP session/rollback groups), `fast` (focused deterministic browser checks), and `network` (externally hosted pages). They are distinct selections, not synonyms. A batch expands groups; a suite names precise test IDs. A test may participate in several suites and batches.

## Acceptance criteria are independently governed

`tests/acceptance/criteria.tsv` tracks requirement ID, title, observable expected behavior, accountable owner, and approval decision (`proposed`, `pending`, `accepted`, `rejected`). `tests/acceptance/links.tsv` maps a criterion to existing catalog test IDs and records whether each is `primary` or `supporting` evidence.

```bash
./scripts/dev.sh test --acceptance-report
./scripts/dev.sh test --acceptance-report --json
./scripts/dev.sh test --acceptance-report --validate
```

A criterion without links is **uncovered**, even when related code/unit tests pass. Linking a test means *evidence exists to be executed*, not that it has passed; passing it does not automatically approve a UAT decision. The initial acceptance register contains 6 proposed criteria from the remote-browser rollout, with **two supporting evidence links** to the mocked container-policy test (`QLT-020`), leaving four requirements uncovered. No primary acceptance evidence or UAT approval is claimed. Owner and decision are intentionally separate from `status` of any test run.

## Runtime metadata and gaps

The runner already produces `run.json` with unique run ID, repository, branch, Git commit and timestamps; `results.jsonl` with per-test `PASS`, `FAIL` or `DISABLED`, elapsed milliseconds and log path; and `report.tsv`, `catalog.json`, `summary.json` and logs. The added scope/size/focus/environment fields flow into JSONL and reports. `DISABLED` means configured off, not `SKIPPED` due to environment. The runner currently does not record a first-class `SKIPPED` or `NOT_RUN` result; acceptance coverage describes unlinked requirements as `uncovered`.

**Metadata still needed before stronger audit/release gating:**

| Suggested field | Purpose | Current state |
| --- | --- | --- |
| `requirement_ids` / acceptance links | Many-to-many product requirement trace | Explicit acceptance mapping exists; currently empty |
| `owner`, `risk`, `priority` | Review/accountability/triage | Criteria have owner; test owners/risk not yet modeled |
| `tags` (e.g. regression, smoke, contract, E2E) | Independent many-valued descriptors, not levels | Not yet modeled; named suites cover execution subsets |
| `isolation`, `cleanup_policy` | Prevent tests destroying active browser state | Described in runner docs/preconditions, not machine-enforced metadata |
| `determinism`, `external_dependencies` | Distinguish controlled fixture from network-dependent flakes | Partially indicated by kind/preconditions/environment |
| `timeout`, `expected_duration`, `resources` | CI scheduling and performance budgets | Actual duration stored per run; limits not formal |
| `platform`, `engine_version`, `chromium_version` | Docker/Podman/Linux compatibility matrix | Not captured in reproducible run metadata |
| `skip_reason`, `quarantine_reason`, `flakiness` | Honest test status and unstable-test management | Disabled lists exist; no first-class reason |
| `evidence_uri`, `artifact_ids` | Screenshots/logs/CDP evidence for acceptance signoff | Log paths exist; normalized artifact links not yet |
| `reviewed_by`, `reviewed_at` | Audit trail before claiming reviewed classification | Only provisional/reviewed status so far |

The key migration rule: **metadata we cannot justify is marked provisional/unclassified, never filled with apparently certain values**.

[Back to Jelly test index](./INDEX.md)

<div align="right"><sub><a href="#top">&uarr; Back to top</a></sub></div>
