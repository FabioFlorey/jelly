<div id="top"></div>

# MCP Surface Benchmarks

**Audience:** Maintainers evaluating changes to the published MCP tool surface.

**Measurement status:** Historical, environment-specific results from the recorded comparison. The original run has no recorded commit hash or date in this document. The repository has since added capabilities, so the numbers below **must not** be interpreted as current tool counts, default configuration or CI acceptance thresholds without rerunning the benchmark.

This report measures the large-surface and small-surface MCP modes from the same Jelly worktree and environment. It is intended to verify that the small-surface Agent API materially reduces published schema cost without introducing an unacceptable local runtime regression.

## Method

Static surface measurements come from the real `jelly::mcp::mcp_tools()` projection, executed in separate subprocesses so each configuration gets its own process-frozen Agent Tool Catalog:

- `large-surface`
- `small-surface` with raw CDP disabled
- `small-surface` with raw CDP enabled

`tools_list_bytes` is the compact JSON byte length of `{"tools":[...]}`, including descriptions, input/output schemas, OAuth security metadata, and MCP metadata.

`approx_context_tokens_4b` is only a coarse 4-bytes-per-token estimate. It is not a tokenizer measurement and should be used only for relative context-footprint comparison.

Runtime measurements use one warm persistent `BrowserSession` against `tests/fixtures/browser-perf.html`, 5 warmup iterations, then 40 samples. Direct semantic primitive vs small-surface semantic `browser-call` comparisons are paired and alternate order on each iteration to reduce ordering/cache bias. Latency numbers are local execution measurements, not network MCP latency.

## Published surface

| Metric | Large-surface | Small-surface raw off | Small-surface raw on |
| --- | ---: | ---: | ---: |
| Published tools | 50 | 15 | 15 |
| Browser-facing entries | 38 | 3 | 3 |
| Browser primitive bindings | 38 | 0 | 0 |
| Builtin facade bindings | 0 | 3 | 3 |
| System-tool bindings | 12 | 12 | 12 |
| `tools/list` JSON bytes | 48,346 | 17,361 | 19,074 |
| Aggregate input-schema bytes | 9,511 | 5,330 | 6,896 |
| Aggregate output-schema bytes | 24,050 | 7,215 | 7,215 |
| Description bytes | 5,036 | 1,862 | 2,009 |
| Approx. 4-byte/token footprint | 12,087 | 4,341 | 4,769 |

Small-surface with raw CDP disabled reduces:

- published tool count by **70%** (50 → 15);
- `tools/list` bytes by **64.1%** (48,346 → 17,361);
- aggregate input-schema bytes by **44.0%**;
- aggregate output-schema bytes by **70.0%**;
- approximate context footprint by **64.1%**.

Enabling raw CDP increases the small-surface `browser-call` schema, but the resulting surface is still **60.5% smaller** than large-surface by `tools/list` bytes.

## Warm local runtime

Latency is intentionally treated as a noisy local microbenchmark. Three paired runs from the same worktree/environment showed substantial absolute variation with host load, so the engineering signal is the large-vs-small relationship rather than one canonical microsecond value.

p50 latency in microseconds:

| Run | Large-surface direct `read-page` | Small-surface `read-page` | Large-surface direct 3-step workflow | Small-surface batched workflow | Raw target | Raw browser |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| A | 860 | 855 | 1,542 | 1,682 | 394 | 188 |
| B | 2,742 | 2,946 | 1,511 | 1,540 | 1,846 | 1,144 |
| C | 705 | 778 | 1,556 | 1,744 | 360 | 296 |

Across these paired runs, small-surface semantic `read-page` p50 ranged from about **0.6% faster to 10.4% slower** than direct primitive execution. The small-surface three-step batch ranged from about **1.9% to 12.1% slower at p50** locally.

The benchmark therefore shows a bounded low-double-digit local wrapper cost rather than a structural runtime regression. In exchange, the representative workflow reduces agent-facing tool round trips from **3 to 1**.

Raw target/browser CDP also remains a low-millisecond-or-better local operation in these runs, but its absolute latency varies with host scheduling and load.

Jelly does not enforce microsecond latency thresholds in CI from this benchmark. CI enforces the stable surface-size properties instead.

## Representative workflow

The benchmark workflow is:

1. mutate the page title;
2. assert the new title;
3. read the page.

Large-surface direct execution requires three independently dispatched semantic primitive operations. Small-surface execution expresses all three in one ordered `browser-call` batch:

```text
large-surface direct agent-facing round trips: 3
small-surface browser-call round trips:       1
reduction:                              66.7%
```

This reduction is separate from CDP command count: small-surface batching changes the agent-facing API round trips while preserving ordered primitive execution internally.

## Acceptance

In the recorded measurement, the small surface was materially smaller by tool count and serialized MCP schema footprint. Across three paired latency runs, the small-surface wrapper stayed within low-double-digit p50 overhead relative to direct primitive execution while the representative workflow cut agent-facing round trips by two thirds. This was the recorded rationale for the initial small-surface cutover; reevaluate the tradeoff using a fresh benchmark when changing the published catalog.

Reproduce with:

```bash
scripts/benchmark-mcp-surface.sh
```

The deterministic quality suite checks surface-budget properties rather than latency: binding distribution, published tool count, total `tools/list` bytes, aggregate input/output-schema bytes, and description bytes. The raw-off and raw-on small-surface variants must retain material headroom versus large-surface, so schema or prose growth cannot hide behind an unchanged tool count. Runtime numbers are reported for engineering decisions but are not treated as hard CI thresholds.

<div align="right"><sub><a href="#top">🡩 Go to the top of the document</a> · <a href="../INDEX.md">Documentation index</a></sub></div>
