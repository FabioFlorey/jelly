#!/usr/bin/env python3
"""Offline routing/slot/final-check evaluation for agent-proposed Jelly tool decisions.

Never invokes Jelly tools or a model and never claims an end-to-end browser OSR.
Use --self-test for deterministic harness smoke tests, or --predictions for real proposals.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
CASES = ROOT / "tests/fixtures/agent-guidance-cases.json"


def load_records(path: Path) -> list[dict[str, Any]]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, list) or any(not isinstance(row, dict) for row in data):
        raise ValueError(f"{path}: expected JSON array of objects")
    return data


def subset(expected: Any, actual: Any) -> bool:
    """Require every expected field, preserving array order and rejecting wrong value types."""
    if isinstance(expected, dict):
        return isinstance(actual, dict) and all(k in actual and subset(v, actual[k]) for k, v in expected.items())
    if isinstance(expected, list):
        return isinstance(actual, list) and len(actual) == len(expected) and all(
            subset(value, actual[i]) for i, value in enumerate(expected)
        )
    return type(expected) is type(actual) and expected == actual


def score(cases: list[dict[str, Any]], predictions: list[dict[str, Any]]) -> dict[str, Any]:
    ids = [c.get("id") for c in cases]
    if len(set(ids)) != len(ids) or any(not isinstance(i, str) for i in ids):
        raise ValueError("duplicate or invalid case ID")
    predicted_ids = [p.get("case_id") for p in predictions]
    if len(set(predicted_ids)) != len(predicted_ids):
        raise ValueError("duplicate prediction case ID")
    unknown = set(predicted_ids) - set(ids)
    if unknown:
        raise ValueError(f"unknown case IDs: {sorted(unknown)}")
    by_id = {p["case_id"]: p for p in predictions}
    results = []
    for c in cases:
        p = by_id.get(c["id"], {})
        route = p.get("tool") == c["expected_tool"] and p.get("tool") not in c.get("avoid", [])
        # Reject unrequested additional actions or an unsafe batch continuation policy.
        slots = route and subset(c["required_args"], p.get("arguments"))
        if p.get("tool") == "browser-call" and isinstance(p.get("arguments"), dict):
            slots = slots and p["arguments"].get("on_error", "stop") != "continue"
        offered_checks = p.get("checks", [])
        checks = (isinstance(offered_checks, list) and all(isinstance(x, str) for x in offered_checks)
                  and set(c["checks"]).issubset(offered_checks))
        results.append({"id": c["id"], "route": route, "slots": slots, "planned_checks": checks, "complete": route and slots and checks})
    n = len(results)
    return {
        "total": n,
        "predicted": len(predictions),
        "tool_selection_accuracy": sum(r["route"] for r in results) / n if n else 0,
        "slot_filling_accuracy": sum(r["slots"] for r in results) / n if n else 0,
        "verification_plan_coverage": sum(r["planned_checks"] for r in results) / n if n else 0,
        "combined_plan_accuracy": sum(r["complete"] for r in results) / n if n else 0,
        "task_outcome_success_rate": None,  # Requires observed browser outcomes, not paper plans.
        "cases": results,
    }


def reference(cases: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Synthetic contract-positive examples, NOT agent/model predictions."""
    return [
        {"case_id": c["id"], "tool": c["expected_tool"], "arguments": c["required_args"], "checks": c["checks"]}
        for c in cases
    ]


def self_test(cases: list[dict[str, Any]]) -> None:
    good = reference(cases)
    assert score(cases, good)["combined_plan_accuracy"] == 1
    bad = [dict(row) for row in good]
    bad[0] = {**bad[0], "tool": "cdp-call"}
    bad[1] = {**bad[1], "arguments": {"action": "schema"}}
    bad[2] = {**bad[2], "checks": []}
    result = score(cases, bad)
    assert result["tool_selection_accuracy"] < 1
    assert result["slot_filling_accuracy"] < 1
    assert result["verification_plan_coverage"] < 1
    assert result["task_outcome_success_rate"] is None
    try:
        score(cases, good + [good[0]])
    except ValueError:
        pass
    else:
        raise AssertionError("duplicate predictions were accepted")
    print(f"PASS: {len(cases)} routing/slot/verification reference cases and negative controls; no browser/model calls")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--self-test", action="store_true", help="Test the scoring harness without an agent")
    group.add_argument("--predictions", type=Path, help="JSON array of agent decisions for these cases")
    group.add_argument("--emit-reference", type=Path, help="Write synthetic examples; not model performance")
    parser.add_argument("--report", type=Path, help="Optional JSON output for evaluated predictions")
    parser.add_argument("--strict", action="store_true", help="Fail unless every case passes all three plan checks")
    args = parser.parse_args()
    cases = load_records(CASES)
    if args.self_test:
        self_test(cases)
        return
    if args.emit_reference:
        args.emit_reference.write_text(json.dumps(reference(cases), indent=2) + "\n", encoding="utf-8")
        print(f"Wrote {len(cases)} synthetic reference decisions to {args.emit_reference}")
        return
    result = score(cases, load_records(args.predictions))
    summary = {k: v for k, v in result.items() if k != "cases"}
    print(json.dumps(summary, indent=2))
    if args.report:
        args.report.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    if args.strict and result["combined_plan_accuracy"] != 1:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
