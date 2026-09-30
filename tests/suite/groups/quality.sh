#!/usr/bin/env bash

quality_fmt() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo fmt --check
}

quality_check() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo check --all-targets
}

quality_tests() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo test --all-targets
}

quality_clippy() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo clippy --all-targets -- -D warnings
}

quality_tool_index() {
  scripts/check-tool-index.sh
}

quality_suite_catalog() {
  local catalog count unique
  catalog="$(tests/suite/run.sh --catalog --json)"
  count="$(jq 'length' <<<"$catalog")"
  jt_assert_gt "$count" "0" "test catalog must contain registered tests"
  unique="$(jq '[.[].id] | unique | length' <<<"$catalog")"
  jt_assert_eq "$unique" "$count" "every test ID must be unique"
  jq -e 'all(.[]; (.id|test("^[A-Z]+-[0-9]{3}$")) and (.group|length>0) and (.name|length>0) and (.description|length>0) and (.preconditions|length>0) and (.input|length>0) and (.expected_output|length>0) and (.kind|length>0) and (.enabled|type=="boolean"))' >/dev/null <<<"$catalog" || jt_fail "every catalog row must satisfy the required metadata contract"
}

quality_suite_shell_syntax() {
  local f
  bash -n tests/suite/run.sh tests/suite/lib.sh tests/suite/manage.sh
  for f in tests/suite/groups/*.sh scripts/check-test-suite.sh scripts/check-browser-session-lifecycle.sh scripts/check-page-runtime.sh scripts/check-framework-mutations.sh scripts/check-ref-semantics.sh scripts/check-semantic-targets.sh scripts/check-shadow-dom.sh scripts/check-filtering-ranking.sh scripts/check-highlight-modes.sh scripts/check-rollback-matrix.sh scripts/check-real-world-regressions.sh; do
    bash -n "$f"
  done
}

quality_suite_lock() {
  local code
  set +e
  tests/suite/run.sh --test REF-001 >/tmp/jelly-suite-lock-probe.log 2>&1
  code=$?
  set -e
  jt_assert_eq "$code" "75" "nested executable suite run must be rejected while parent run owns global lock"
  grep -q 'already active' /tmp/jelly-suite-lock-probe.log || jt_fail "lock rejection must explain that another suite run is active"
  rm -f /tmp/jelly-suite-lock-probe.log
}

quality_test_index() {
  local group batch groups
  [[ -f tests/INDEX.md ]] || jt_fail "tests/INDEX.md must exist"
  grep -Fq '[`suite/README.md`](./suite/README.md)' tests/INDEX.md || jt_fail "test index must link to suite/README.md"

  while IFS= read -r group; do
    [[ -n "$group" ]] || continue
    grep -Fq "| \`$group\` |" tests/INDEX.md || jt_fail "test index is missing group: $group"
  done < <(tests/suite/run.sh --list | awk -F '\t' 'NR>1 {print $2}' | sort -u)

  while IFS=$'\t' read -r batch groups; do
    [[ -n "$batch" && "${batch#\#}" == "$batch" ]] || continue
    grep -Fq "| \`$batch\` |" tests/INDEX.md || jt_fail "test index is missing batch: $batch"
  done < tests/suite/config/batches.tsv
}

jt_register "QLT-001" "quality" "Rust formatting" "Verify all Rust source files conform to rustfmt without modifying them." "Rust toolchain and rustfmt installed." "cargo fmt --check" "Command exits 0 and produces no formatting diff." "quality" quality_fmt
jt_register "QLT-002" "quality" "All-target compilation" "Compile every library, binary, test, and auxiliary target in check mode." "Rust dependencies are available; /data/.jelly-build is writable." "cargo check --all-targets" "All targets compile successfully." "quality" quality_check
jt_register "QLT-003" "quality" "Rust test suite" "Run all Rust unit and binary tests across all targets." "Project compiles and test dependencies are available." "cargo test --all-targets" "Every Rust test passes with zero failures." "quality" quality_tests
jt_register "QLT-004" "quality" "Clippy warnings as errors" "Run Clippy across all targets and reject every warning." "Clippy component is installed." "cargo clippy --all-targets -- -D warnings" "Clippy exits 0 with no warnings." "quality" quality_clippy
jt_register "QLT-005" "quality" "Generated tool index consistency" "Verify the checked-in agent tool index matches the registry-derived generated output." "Jelly binaries can be built and scripts/check-tool-index.sh is executable." "scripts/check-tool-index.sh" "Generated and checked-in tool indexes are identical." "quality" quality_tool_index
jt_register "QLT-006" "quality" "Test catalog metadata integrity" "Verify the executable test catalog has unique stable IDs and complete required metadata for every registered test." "tests/suite runner and jq available." "tests/suite/run.sh --catalog --json" "Catalog is non-empty; IDs are unique and match PREFIX-NNN; every required metadata field is non-empty and enabled is boolean." "quality" quality_suite_catalog
jt_register "QLT-007" "quality" "Test harness shell syntax" "Verify the runner, group implementations, and compatibility wrappers are syntactically valid Bash." "bash available." "bash -n on suite and wrapper scripts" "Every test-system shell file parses successfully." "quality" quality_suite_shell_syntax
jt_register "QLT-008" "quality" "Global suite concurrency lock" "Verify executable suite runs are serialized so shared browser/service state cannot be corrupted by concurrent runs." "Test is executed by tests/suite/run.sh while the parent runner owns the lock." "Attempt a nested executable suite run." "Nested run exits 75 and reports that another suite run is already active." "quality" quality_suite_lock
jt_register "QLT-009" "quality" "Test index coverage" "Verify the repository test index exists and stays synchronized with executable groups and named batches." "tests/INDEX.md, suite catalog, and batches.tsv are readable." "Compare documented group/batch rows with run.sh --list and config/batches.tsv." "Every executable group and configured batch is present in tests/INDEX.md, which links to the detailed suite guide." "quality" quality_test_index
