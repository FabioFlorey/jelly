<div id="top"></div>

# Agent Guidance Evaluation

**Audience:** Maintainers evaluating changes to MCP instructions, tool descriptions, and tool-selection strategies.

Jelly's instructions are an executable operating specification, not a promise of universal model performance. Evaluate **global instructions + current MCP schemas + selection behavior together**, using the [tool-selection playbook](../guides/AGENT_PLAYBOOK.md) and actual published contracts. The evaluator below is offline, requires no browser or credentials, and **does not** execute an LLM.

## Deterministic evaluator

The [routing scenarios](../../tests/fixtures/agent-guidance-cases.json) cover small and large MCP surfaces, discovery, named arguments, targeted inspection, stale refs, bounded verification, screenshots versus recordings, artifacts, routines, event subscriptions, side-effect safety, and Telegram delivery. Each case includes an intent, expected tool, required argument subset, required final-check plan, and discouraged alternatives.

Run the harness self-test and generate a *synthetic* reference response (not a measured agent result):

```bash
cargo run --locked --bin jelly-maint -- check agent-guidance --self-test
cargo run --locked --bin jelly-maint -- check agent-guidance --emit-reference /tmp/jelly-reference.json
cargo run --locked --bin jelly-maint -- check agent-guidance --predictions /tmp/jelly-reference.json --strict
```

For a **real model evaluation**, give the cases' `intent` and `surface` to the model along with the actual server instructions and corresponding published `tools/list` schemas. Capture each decision as a JSON array of records:

```json
[
  {
    "case_id": "AGT-001",
    "tool": "browser-schema",
    "arguments": {"action": "search", "query": "find interactive control"},
    "checks": ["load-schema-before-action"]
  }
]
```

Then run:

```bash
cargo run --locked --bin jelly-maint -- check agent-guidance --predictions /tmp/agent-decisions.json --report /tmp/jelly-routing-report.json
```

The scorer reports **tool-selection accuracy**, **slot-filling accuracy** (required argument subsets, gated on the correct tool), **verification-plan coverage**, and **combined plan accuracy**. Missing scenarios count as failures. Exact-match case expectations are intentionally narrow: report alternate but valid plans separately for human review. These scores measure *plans*, not successful execution; `task_outcome_success_rate` is `null` because no browser outcome is observed by this offline scorer. Do **not** publish synthetic reference scores as model results or call them OSR.

## End-to-end task outcomes and improvement loop

To measure actual task success, run agent plans against isolated fixtures or a controlled live-browser environment. Log case ID, agent/model identifier, prompt and schema revision, initial browser state, tool name and validated arguments, structured result/error, retries, final assertions, artifact IDs and verification status, and elapsed time. **Redact cookies, access tokens, credentials, user content, and full private pages before storing logs.** Keep raw logs under the runtime directory, not in the repository.

A recommended cycle:

1. Record a baseline across the same tasks, model version, published tool surface, and budgets.
2. Classify failures separately as misrouting, invalid arguments, stale observations, unsafe retry, missing final verification, or blocked site state.
3. Change the smallest relevant instructions and/or schema descriptions. Prefer short *when to use / when not to use* distinctions over longer generic prose.
4. Rerun both the offline scenario corpus and identical browser tasks. Compare tool selection, slot filling, verified task completion, latency, tokens, and unnecessary calls against the baseline.
5. Keep changes only with observed improvement and no new contract conflicts. Expand the corpus with reproducible failures. Evaluate across different model families before declaring a description universally beneficial.

Source-of-truth rules: `tools/list` and `inputSchema` are authoritative. `browser-schema` discovers semantic operations only when advertised; local `agent-discover` is a separate CLI plane. Check the [MCP surface benchmarks](./MCP_SURFACE_BENCHMARKS.md) for payload-cost tradeoffs; those figures measure tool-surface overhead, **not** agent routing accuracy.

<div align="right"><sub><a href="#top">&uarr; Back to top</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
