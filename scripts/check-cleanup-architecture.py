#!/usr/bin/env python3
"""Mechanical architecture guardrails established by the cleanup refactor."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
failures: list[str] = []


def fail(message: str) -> None:
    failures.append(message)


def production_prefix(path: Path) -> str:
    text = path.read_text()
    marker = "#[cfg(test)]\n"
    return text.split(marker, 1)[0]


primitive_patterns = {
    r'ok_or\("[^"]*"\)': 'plain ok_or string',
    r'Err\("[^"]*"\.into\(\)\)': 'plain string Err conversion',
    r'Err\(format!\(': 'plain formatted Err conversion',
}
for path in sorted((ROOT / "src/primitives").glob("*.rs")):
    text = path.read_text()
    for pattern, label in primitive_patterns.items():
        for match in re.finditer(pattern, text):
            line = text.count("\n", 0, match.start()) + 1
            fail(f"{path.relative_to(ROOT)}:{line}: {label} bypasses typed primitive errors")

direct_fs = re.compile(
    r"fs::(?:read|read_to_string|write|create_dir_all|metadata|canonicalize|File::open)[^;\n]*\?"
)
for path in sorted((ROOT / "src/primitives").glob("*.rs")):
    text = path.read_text()
    for match in direct_fs.finditer(text):
        line = text.count("\n", 0, match.start()) + 1
        fail(
            f"{path.relative_to(ROOT)}:{line}: direct filesystem ? in primitive must map "
            "user-facing vs internal failure explicitly"
        )

surface_files = [
    ROOT / "src/agent/surface.rs",
    ROOT / "src/agent/catalog_config.rs",
    ROOT / "src/agent/catalog_cache.rs",
    ROOT / "src/agent/catalog_build.rs",
    ROOT / "src/agent/catalog.rs",
]
for path in surface_files:
    text = production_prefix(path)
    forbidden = [
        (r'"legacy"', 'legacy MCP surface literal'),
        (r'"compact"', 'compact MCP surface literal'),
        (r"\bLegacy\b", 'Legacy MCP surface identifier'),
        (r"\bCompact\b", 'Compact MCP surface identifier'),
        (r"\blegacy_agent_catalog\b", 'legacy agent catalog alias'),
        (r"\bcompact_agent_catalog\b", 'compact agent catalog alias'),
    ]
    for pattern, label in forbidden:
        match = re.search(pattern, text)
        if match:
            line = text.count("\n", 0, match.start()) + 1
            fail(f"{path.relative_to(ROOT)}:{line}: forbidden {label}")

lib = (ROOT / "src/lib.rs").read_text()
private_modules = ["agent", "artifacts", "browser", "error", "execution", "mcp_auth", "primitives"]
public_modules = ["browser_launcher", "mcp", "recording", "routine"]
for module in private_modules:
    if re.search(rf"(?m)^pub\s+mod\s+{re.escape(module)}\s*;", lib):
        fail(f"src/lib.rs: implementation module {module} became public")
    if not re.search(rf"(?m)^mod\s+{re.escape(module)}\s*;", lib):
        fail(f"src/lib.rs: expected private implementation module {module} is missing")
for module in public_modules:
    if not re.search(rf"(?m)^pub\s+mod\s+{re.escape(module)}\s*;", lib):
        fail(f"src/lib.rs: required public package namespace {module} is missing")

if failures:
    print("cleanup architecture guardrails: FAIL")
    for failure in failures:
        print(f"- {failure}")
    raise SystemExit(1)

print("cleanup architecture guardrails: PASS")
