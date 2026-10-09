#!/usr/bin/env bash

quality_fmt() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo fmt --check
  cargo fmt --manifest-path scripts/test-jelly-wrapper/Cargo.toml -- --check
  cargo fmt --manifest-path tests/suite/support/event-probe/Cargo.toml -- --check
  cargo fmt --manifest-path tests/suite/support/agent-api-probe/Cargo.toml -- --check
}

quality_check() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo check --all-targets
  cargo check --quiet --locked \
    --manifest-path scripts/test-jelly-wrapper/Cargo.toml \
    --target-dir $CONFIG_RUNTIME_ROOT/test-jelly-wrapper-target
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo check --quiet --locked \
    --manifest-path tests/suite/support/event-probe/Cargo.toml
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo check --quiet --locked \
    --manifest-path tests/suite/support/agent-api-probe/Cargo.toml
}

quality_tests() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo test --all-targets
}

quality_clippy() {
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo clippy --all-targets -- -D warnings
  cargo clippy --quiet --locked \
    --manifest-path scripts/test-jelly-wrapper/Cargo.toml \
    --target-dir $CONFIG_RUNTIME_ROOT/test-jelly-wrapper-target -- -D warnings
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo clippy --quiet --locked \
    --manifest-path tests/suite/support/event-probe/Cargo.toml -- -D warnings
  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo clippy --quiet --locked \
    --manifest-path tests/suite/support/agent-api-probe/Cargo.toml -- -D warnings
}

quality_tool_index() {
  scripts/check-tool-index.sh
}

quality_cleanup_architecture() {
  cargo run --quiet --locked --bin jelly-maint -- check architecture
}

quality_mcp_build_root() {
  local script
  for script in scripts/install-mcp-services.sh scripts/run-mcp-hosting.sh scripts/run-nip-io.sh scripts/clean-runtime.sh; do
    grep -Fq 'BUILD_DIR="$CONFIG_BUILD_ROOT"' "$script" \
      || jt_fail "$script must resolve BUILD_DIR from CONFIG_BUILD_ROOT"
  done
  ! grep -Eq 'BUILD_DIR=.*\.jelly-build' scripts/install-mcp-services.sh \
    || jt_fail "install-mcp-services.sh must not derive a separate .jelly-build path"
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
  local file
  while IFS= read -r file; do
    bash -n "$file" || return
  done < <(git ls-files '*.sh')
}

quality_shell_safety() {
  cargo run --quiet --locked --bin jelly-maint -- check security
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

quality_test_jelly_cli() {
  local help catalog rust
  help="$(cargo test-jelly -h)"
  grep -Fq 'Tests · help' <<<"$help" || jt_fail "test-jelly help must render the centered help heading"
  grep -Fq 'cargo test-jelly [CARGO_TEST_ARGS...]' <<<"$help" || jt_fail "test-jelly help must document cargo argument forwarding"
  grep -Fq 'cargo test-jelly --catalog' <<<"$help" || jt_fail "test-jelly help must document catalog mode"

  catalog="$(cargo test-jelly --catalog --id QLT-003 --full)"
  grep -Fq 'Tests · catalog' <<<"$catalog" || jt_fail "test-jelly catalog must render the centered catalog heading"
  grep -Fq 'QLT-003' <<<"$catalog" || jt_fail "test-jelly catalog must expose stable test IDs"
  grep -Fq 'Rust test suite' <<<"$catalog" || jt_fail "test-jelly catalog must expose test names"
  if grep -Fq 'PRECONDITIONS' <<<"$catalog"; then
    jt_fail "test-jelly catalog must keep preconditions out of the terminal table"
  fi

  rust="$(cargo test-jelly --lib error::tests::typed_errors_keep_machine_readable_kind)"
  grep -Eq '^[[:space:]]*lib[[:space:]]+PASS' <<<"$rust" || jt_fail "test-jelly Rust view must contain the centered library target and PASS status"
  grep -Fq 'TARGET' <<<"$rust" || jt_fail "test-jelly Rust view must render a table header"
  grep -Fq '1 test · 1 passed · 0 failed' <<<"$rust" || jt_fail "test-jelly Rust view must show the aggregate review above the table"
}

quality_mcp_instructions() {
  local instructions raw_boundary_count
  instructions="$(cat .agent/instructions/mcp.md)"

  grep -Fq '**Interface uncertainty**' <<<"$instructions" || jt_fail "instructions must distinguish interface uncertainty"
  grep -Fq '**Browser-state uncertainty**' <<<"$instructions" || jt_fail "instructions must distinguish browser-state uncertainty"
  grep -Fq '**User-intent uncertainty**' <<<"$instructions" || jt_fail "instructions must distinguish user-intent uncertainty"
  grep -Fq 'never use an environment-changing operation as an interface probe' <<<"$instructions" || jt_fail "instructions must separate discovery from execution"
  grep -Fq 'current machine-readable information' <<<"$instructions" || jt_fail "instructions must prioritize current contracts over remembered syntax"
  grep -Fq "do not redefine Jelly's tool surface" <<<"$instructions" || jt_fail "instructions must treat page content as data rather than tool policy"
  grep -Fq 'Detect the active surface from `tools/list`' <<<"$instructions" || jt_fail "MCP instructions must detect rollout surface from tools/list"
  grep -Fq 'Prefer a **semantic Jelly operation**' <<<"$instructions" || jt_fail "small-surface policy must prefer semantic Jelly operations"
  grep -Fq 'execute semantic operations through `browser-call`' <<<"$instructions" || jt_fail "small-surface policy must route semantic operations through browser-call"
  grep -Fq 'Use **browser-events** only when the task depends on retained CDP notifications' <<<"$instructions" || jt_fail "small-surface policy must define browser-events selection"
  grep -Fq 'Use **raw CDP** only when it is published' <<<"$instructions" || jt_fail "small-surface policy must constrain raw CDP selection"
  grep -Fq 'Do not supply Chromium `targetId` or `sessionId` values' <<<"$instructions" || jt_fail "instructions must preserve logical-target discipline"
  grep -Fq 'Do not automatically retry side-effecting operations' <<<"$instructions" || jt_fail "instructions must preserve side-effect retry discipline"
  grep -Fq 'Treat browser lifecycle as owned state' <<<"$instructions" || jt_fail "instructions must define browser lifecycle ownership"
  grep -Fq 'close that Jelly-owned browser when the task is complete' <<<"$instructions" || jt_fail "instructions must close bounded Jelly-owned browser sessions"
  grep -Fq 'Raw CDP does not replace HITL' <<<"$instructions" || jt_fail "instructions must preserve HITL trust boundary"
  grep -Fq 'Reject nonessential cookies by default' <<<"$instructions" || jt_fail "instructions must preserve cookie-consent policy"
  grep -Fq 'agent-discover schema <tool>' <<<"$instructions" || jt_fail "CLI instructions must prefer schema discovery over argument probing"
  grep -Fq 'agent-run type-text -- --help' <<<"$instructions" || jt_fail "CLI instructions must document literal dash-prefixed arguments"
  grep -Fq 'Continuous mode streams captured renderer frames into FFmpeg' <<<"$instructions" || jt_fail "instructions must expose FFmpeg-backed continuous recording"
  grep -Fq 'automatically derives a short action comment from trace metadata' <<<"$instructions" || jt_fail "instructions must expose automatic recording commentary"
  grep -Fq 'stores it as the step `label`' <<<"$instructions" || jt_fail "instructions must expose recording label metadata"

  if grep -Fq 'Use direct primitives for short, local interactions' <<<"$instructions"; then
    jt_fail "small-surface instructions must not tell MCP agents to select individually published browser primitives"
  fi
  if grep -Fq 'Use snapshot-interactive before clicking' <<<"$instructions"; then
    jt_fail "small-surface instructions must not depend on snapshot-interactive being a top-level MCP tool"
  fi

  raw_boundary_count="$(grep -c '^### Raw CDP trust boundary$' docs/reference/MCP.md)"
  jt_assert_eq "$raw_boundary_count" "1" "MCP documentation must contain exactly one raw-CDP trust-boundary section"
}

quality_agent_guidance() {
  grep -Fq '### Tool choice in one pass' .agent/instructions/mcp.md \
    || jt_fail 'agent startup instructions must contain a tool-selection decision table'
  grep -Fq '### Final task check' .agent/instructions/mcp.md \
    || jt_fail 'agent startup instructions must verify user outcomes before reporting success'
  cargo run --quiet --locked --bin jelly-maint -- check agent-guidance --self-test
}

quality_raw_cdp_boundary_docs() {
  local reliability security mcp config_file
  reliability="$(cat docs/guides/RELIABILITY.md)"
  security="$(cat SECURITY.md)"
  mcp="$(cat docs/reference/MCP.md)"
  config_file="$(cat config/jelly.toml)"

  jt_assert_eq "$(grep -c '^Raw CDP has a different reliability contract from Jelly semantic operations\.' docs/guides/RELIABILITY.md)" "1"     "Reliability must contain exactly one raw-CDP reliability-contract paragraph"
  grep -Fq 'authorization boundary is deployment-wide, not per-client' <<<"$security" || jt_fail "Security docs must state the current raw-CDP authorization boundary"
  grep -Fq 'all published MCP tools use the same OAuth `jelly` scope' <<<"$security" || jt_fail "Security docs must state the single OAuth scope"
  grep -Fq 'Changing `[mcp].raw_cdp` requires restarting the MCP process' <<<"$security" || jt_fail "Security docs must state restart semantics"
  grep -Fq 'The setting is process-wide and takes effect at MCP startup' <<<"$mcp" || jt_fail "MCP docs must state process-wide startup semantics"
  grep -Fq 'changing `[mcp].raw_cdp` requires an MCP restart' <<<"$mcp" || jt_fail "MCP docs must state restart requirement"
  grep -Fq 'raw-CDP enablement is not per-client' <<<"$mcp" || jt_fail "MCP docs must state current per-client limitation"
  grep -Fq 'one process-wide `BrowserSession`' <<<"$mcp" || jt_fail "MCP docs must state process-wide persistent BrowserSession semantics"
  grep -Fq 'not isolated per OAuth client' <<<"$mcp" || jt_fail "MCP docs must state that event/session state is not per-client"
  grep -Fq 'subscription IDs, is likewise process-wide' <<<"$security" || jt_fail "Security docs must state process-wide subscription state"
  grep -Fq 'raw_cdp = false' <<<"$config_file" || jt_fail "config must keep raw CDP disabled by default"
}

quality_discovery_docs() {
  local index discovery instructions
  index="$(cat .agent/tools/index.md)"
  discovery="$(cat docs/reference/DISCOVERY.md)"
  instructions="$(cat .agent/instructions/mcp.md)"

  grep -Fq '# jelly internal capability index' <<<"$index" || jt_fail "generated index must identify itself as the internal capability index"
  grep -Fq 'not the published MCP Agent API catalog' <<<"$index" || jt_fail "generated index must disclaim MCP publication authority"
  grep -Fq 'active remote MCP surface is defined by `tools/list`' <<<"$index" || jt_fail "generated index must point remote clients to tools/list"
  grep -Fq 'small-surface browser-operation discovery uses `browser-schema`' <<<"$index" || jt_fail "generated index must point small-surface discovery to browser-schema"
  grep -Fq '## Internal discovery CLI' <<<"$index" || jt_fail "generated index must label agent-discover as internal discovery"

  grep -Fq '# Capability and Agent API Discovery' <<<"$discovery" || jt_fail "discovery docs must name both capability and Agent API discovery"
  grep -Fq '## Internal capability discovery' <<<"$discovery" || jt_fail "discovery docs must describe internal discovery"
  grep -Fq '## Published Agent API discovery' <<<"$discovery" || jt_fail "discovery docs must describe published Agent API discovery"
  grep -Fq 'Remote MCP clients must treat `tools/list` as the authoritative published Agent API' <<<"$discovery" || jt_fail "discovery docs must make tools/list authoritative"
  grep -Fq 'Use the published `browser-schema` tool' <<<"$discovery" || jt_fail "discovery docs must direct small-surface remote discovery to browser-schema"
  grep -Fq 'Do not use the generated internal capability index as a substitute for `tools/list`' <<<"$discovery" || jt_fail "discovery docs must separate internal and remote discovery"

  grep -Fq 'Generated internal tool index' <<<"$instructions" || jt_fail "operating instructions must classify .agent/tools/index.md as internal"
  grep -Fq 'tools/list` as the authoritative remote surface' <<<"$instructions" || jt_fail "operating instructions must keep tools/list authoritative"
}

quality_aux_lock_alignment() {
  cargo run --quiet --locked \
    --manifest-path scripts/test-jelly-wrapper/Cargo.toml \
    --target-dir $CONFIG_RUNTIME_ROOT/test-jelly-wrapper-target \
    --bin check-test-lock-alignment
}

quality_mcp_surface_budget() {
  local report large_bytes small_bytes small_raw_bytes large_tools small_tools
  local large_input small_input small_raw_input large_output small_output small_raw_output
  local large_desc small_desc small_raw_desc

  CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo build --quiet --locked     --manifest-path tests/suite/support/agent-api-probe/Cargo.toml
  report="$("$BIN_DIR/jelly-agent-api-probe" surface-report)"

  large_bytes="$(jq -r '.large_surface.tools_list_bytes' <<<"$report")"
  small_bytes="$(jq -r '.small_surface_raw_off.tools_list_bytes' <<<"$report")"
  small_raw_bytes="$(jq -r '.small_surface_raw_on.tools_list_bytes' <<<"$report")"
  large_tools="$(jq -r '.large_surface.tool_count' <<<"$report")"
  small_tools="$(jq -r '.small_surface_raw_off.tool_count' <<<"$report")"
  large_input="$(jq -r '.large_surface.aggregate_input_schema_bytes' <<<"$report")"
  small_input="$(jq -r '.small_surface_raw_off.aggregate_input_schema_bytes' <<<"$report")"
  small_raw_input="$(jq -r '.small_surface_raw_on.aggregate_input_schema_bytes' <<<"$report")"
  large_output="$(jq -r '.large_surface.aggregate_output_schema_bytes' <<<"$report")"
  small_output="$(jq -r '.small_surface_raw_off.aggregate_output_schema_bytes' <<<"$report")"
  small_raw_output="$(jq -r '.small_surface_raw_on.aggregate_output_schema_bytes' <<<"$report")"
  large_desc="$(jq -r '.large_surface.description_bytes' <<<"$report")"
  small_desc="$(jq -r '.small_surface_raw_off.description_bytes' <<<"$report")"
  small_raw_desc="$(jq -r '.small_surface_raw_on.description_bytes' <<<"$report")"

  jt_assert_gt "$large_tools" "$small_tools" "small-surface must publish fewer tools than large-surface"
  jt_assert_eq "$(jq -r '.large_surface.binding_distribution.browser_primitive' <<<"$report")" "47"     "large-surface must publish the 47 semantic browser primitives"
  jt_assert_eq "$(jq -r '.large_surface.binding_distribution.builtin_facade' <<<"$report")" "0"     "large-surface must not publish small-surface browser facade entries"
  jt_assert_eq "$(jq -r '.small_surface_raw_off.binding_distribution.browser_primitive' <<<"$report")" "0"     "small-surface must not republish individual browser primitives"
  jt_assert_eq "$(jq -r '.small_surface_raw_off.binding_distribution.builtin_facade' <<<"$report")" "3"     "small-surface must publish exactly the three browser facade entries"
  jt_assert_eq "$(jq -r '.small_surface_raw_on.binding_distribution.builtin_facade' <<<"$report")" "3"     "raw CDP enablement must not add top-level facade tools"
  jt_assert_eq "$(jq -r '.large_surface.system_tool_count' <<<"$report")" "$(jq -r '.small_surface_raw_off.system_tool_count' <<<"$report")"     "small-surface and large-surface must preserve the same system-tool count"

  (( small_bytes * 2 <= large_bytes )) || jt_fail "small-surface raw-off tools/list must be at least 50% smaller than large-surface"
  (( small_raw_bytes * 2 <= large_bytes )) || jt_fail "small-surface raw-on tools/list must remain at least 50% smaller than large-surface"
  (( small_input * 100 <= large_input * 65 )) || jt_fail "small-surface raw-off input schemas must remain at least 35% smaller than large-surface"
  (( small_raw_input * 100 <= large_input * 80 )) || jt_fail "small-surface raw-on input schemas must remain at least 20% smaller than large-surface"
  (( small_output * 100 <= large_output * 40 )) || jt_fail "small-surface raw-off output schemas must remain at least 60% smaller than large-surface"
  (( small_raw_output * 100 <= large_output * 40 )) || jt_fail "small-surface raw-on output schemas must remain at least 60% smaller than large-surface"
  (( small_desc * 2 <= large_desc )) || jt_fail "small-surface raw-off descriptions must remain at least 50% smaller than large-surface"
  (( small_raw_desc * 2 <= large_desc )) || jt_fail "small-surface raw-on descriptions must remain at least 50% smaller than large-surface"
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
jt_register "QLT-002" "quality" "All-target compilation" "Compile every library, binary, test, and auxiliary target in check mode." "Rust dependencies are available; the configured build root is writable." "cargo check --all-targets" "All targets compile successfully." "quality" quality_check
jt_register "QLT-003" "quality" "Rust test suite" "Run all Rust unit and binary tests across all targets." "Project compiles and test dependencies are available." "cargo test --all-targets" "Every Rust test passes with zero failures." "quality" quality_tests
jt_register "QLT-004" "quality" "Clippy warnings as errors" "Run Clippy across all targets and reject every warning." "Clippy component is installed." "cargo clippy --all-targets -- -D warnings" "Clippy exits 0 with no warnings." "quality" quality_clippy
jt_register "QLT-005" "quality" "Generated tool index consistency" "Verify the checked-in agent tool index matches the registry-derived generated output." "Jelly binaries can be built and scripts/check-tool-index.sh is executable." "scripts/check-tool-index.sh" "Generated and checked-in tool indexes are identical." "quality" quality_tool_index
jt_register "QLT-006" "quality" "Test catalog metadata integrity" "Verify the executable test catalog has unique stable IDs and complete required metadata for every registered test." "tests/suite runner and jq available." "tests/suite/run.sh --catalog --json" "Catalog is non-empty; IDs are unique and match PREFIX-NNN; every required metadata field is non-empty and enabled is boolean." "quality" quality_suite_catalog
jt_register "QLT-007" "quality" "Test harness shell syntax" "Verify the runner, group implementations, and compatibility wrappers are syntactically valid Bash." "bash available." "bash -n on suite and wrapper scripts" "Every test-system shell file parses successfully." "quality" quality_suite_shell_syntax
jt_register "QLT-008" "quality" "Global suite concurrency lock" "Verify executable suite runs are serialized so shared browser/service state cannot be corrupted by concurrent runs." "Test is executed by tests/suite/run.sh while the parent runner owns the lock." "Attempt a nested executable suite run." "Nested run exits 75 and reports that another suite run is already active." "quality" quality_suite_lock
jt_register "QLT-009" "quality" "Test index coverage" "Verify the repository test index exists and stays synchronized with executable groups and named batches." "tests/INDEX.md, suite catalog, and batches.tsv are readable." "Compare documented group/batch rows with run.sh --list and config/batches.tsv." "Every executable group and configured batch is present in tests/INDEX.md, which links to the detailed suite guide." "quality" quality_test_index
jt_register "QLT-010" "quality" "test-jelly CLI contract" "Verify the cargo test-jelly frontend documents argument forwarding, renders canonical behavioral-test metadata, and presents Rust test summaries in table form." "Cargo alias, the suite catalog, and Rust test binaries are available." "Run cargo test-jelly help, catalog QLT-003, and one focused Rust unit test." "Help documents both modes; catalog exposes metadata; focused Rust test renders PASS and preserves counts." "quality" quality_test_jelly_cli
jt_register "QLT-011" "quality" "Small-surface MCP operating instructions" "Verify the agent operating instructions teach small-surface discovery and execution without depending on individually published browser primitives, while preserving verification, retry, HITL, logical-target, raw-CDP, and cookie discipline." ".agent/instructions/mcp.md and docs/reference/MCP.md are readable." "Inspect required small-surface policy guidance, reject large-surface primitive-selection wording, and verify a single raw-CDP trust-boundary section." "Instructions select semantic operations via browser-schema/browser-call, use browser-events/raw CDP only for their intended roles, retain safety/reliability discipline, and do not depend on top-level browser primitives." "quality" quality_mcp_instructions
jt_register "QLT-012" "quality" "Discovery-layer documentation boundary" "Verify generated/internal capability documentation cannot be mistaken for the published MCP Agent API and that remote discovery points to tools/list and browser-schema." ".agent/tools/index.md, docs/reference/DISCOVERY.md, and .agent/instructions/mcp.md are readable and the generated index is current." "Inspect boundary wording across the generated internal index, discovery documentation, and operating instructions." "Internal registry docs are explicitly non-MCP; tools/list is authoritative remotely; browser-schema is small-surface semantic discovery; agent-discover remains internal CLI/development discovery." "quality" quality_discovery_docs
jt_register "QLT-013" "quality" "Raw CDP authorization documentation boundary" "Verify the raw-CDP documentation states the current process-wide authorization model, restart semantics, and a single reliability contract." "SECURITY.md, docs/reference/MCP.md, docs/guides/RELIABILITY.md, and .env.example are readable." "Inspect process-wide/single-scope/restart wording and ensure the reliability contract is not duplicated." "Raw CDP is documented as deployment-wide under the current jelly OAuth scope, configuration changes require MCP restart, and Reliability contains one canonical contract paragraph." "quality" quality_raw_cdp_boundary_docs
jt_register "QLT-014" "quality" "Auxiliary Rust lockfile alignment" "Verify Rust test probes that depend on Jelly resolve the exact same registry package versions/checksums as the root build." "Cargo.lock and the event/Agent API probe lockfiles are readable; the Rust tooling wrapper builds." "Compare the complete registry package tuple set (name, version, source, checksum) in each probe lock against Cargo.lock." "Both probe lockfiles have exactly the same registry dependency resolution as the root lock; probe Cargo commands run with --locked." "quality" quality_aux_lock_alignment
jt_register "QLT-015" "quality" "Small-surface MCP budget" "Verify the small-surface MCP Agent API remains materially smaller than large-surface using the real tools/list projection." "Agent API probe builds with the root-aligned lockfile; jq is available." "Measure large-surface, small-surface raw-off, and small-surface raw-on tools/list JSON using jelly::mcp::mcp_tools in isolated subprocesses." "Small-surface preserves the intended binding distribution and system-tool count while keeping total tools/list, input/output schemas, and descriptions materially smaller than large-surface." "quality" quality_mcp_surface_budget
jt_register "QLT-016" "quality" "Cleanup architecture guardrails" "Verify cleanup-established architecture conventions cannot silently regress." "Rust/Cargo and repository sources are available." "Run jelly-maint check architecture against primitive errors, MCP surface naming, and lib.rs module visibility." "Primitive failures remain explicitly typed, only canonical MCP surface names appear in production surface code, and implementation modules remain private." "quality" quality_cleanup_architecture
jt_register "QLT-017" "quality" "Canonical MCP build root" "Verify MCP install/hosting/runtime scripts consume the shared Cargo build root instead of deriving a second target directory." "scripts/config.sh exposes CONFIG_BUILD_ROOT and MCP service scripts are readable." "Inspect install, hosting, nip.io, and cleanup scripts for BUILD_DIR resolution." "All MCP service scripts resolve BUILD_DIR from CONFIG_BUILD_ROOT and the installer contains no separate .jelly-build derivation." "quality" quality_mcp_build_root

jt_register "QLT-018" "quality" "Agent routing guidance evaluation" "Validate the compact MCP decision table, final-outcome gate, and routing/slot/verification scoring harness with negative controls."   ".agent/instructions/mcp.md and agent-guidance fixtures are readable; Rust/Cargo available."   "Check bootstrap rules and run jelly-maint check agent-guidance --self-test."   "All deterministic reference cases and mutation negatives behave correctly without model or browser calls." "quality" quality_agent_guidance

jt_register "QLT-019" "quality" "Shell environment and cleanup safety" "Verify untrusted dotenv values stay inert, environment files are private, and unsafe deletion targets are rejected." "Rust/Cargo, Bash and coreutils available." "Run isolated dotenv and cleanup-guard regression fixtures without modifying runtime state." "Hostile values round-trip as data and broad or non-Jelly deletion paths fail closed." "quality" quality_shell_safety
