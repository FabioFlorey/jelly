#!/usr/bin/env python3
"""Offline documentation checks for Jelly's GitHub-flavored Markdown.

Checks local Markdown/HTML links, section anchors, the document index,
JSON/TOML code examples, Mermaid block headers, and selected config-drift
warnings. External URLs are deliberately not fetched.
"""
from __future__ import annotations

from collections import defaultdict
from pathlib import Path
from urllib.parse import unquote, urlsplit
import json
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
LINK = re.compile(r"\]\(([^)\n]+)\)|<a\b[^>]*\bhref=\"([^\"]+)\"", re.I)
BLOCK = re.compile(r"^\s*```([^\n]*)\n(.*?)^\s*```\s*$", re.M | re.S)
HEAD = re.compile(r"^#{1,6}\s+(.+?)\s*#*\s*$", re.M)
TOP_ID = re.compile(r"\bid=\"([^\"]+)\"")
GITHUB_DOC_URL = re.compile(
    r"https://github\.com/FabioFlorey/jelly/(?:blob|tree)/main/(docs(?:/[^\s\"<>)]*)?)"
)


def slug(text: str) -> str:
    """Approximate GitHub heading IDs for simple documentation headings."""
    text = re.sub(r"<[^>]*>", "", text).replace("`", "").strip().lower()
    text = "".join(c for c in text if c.isalnum() or c in " -_")
    return re.sub(r"\s+", "-", text)


def anchors(markdown: str) -> set[str]:
    counts: dict[str, int] = defaultdict(int)
    found = set(TOP_ID.findall(markdown))
    for header in HEAD.findall(markdown):
        name = slug(header)
        suffix = counts[name]
        counts[name] += 1
        found.add(name if suffix == 0 else f"{name}-{suffix}")
    return found


def check() -> None:
    documents = sorted(DOCS.rglob("*.md"))
    assert len(documents) >= 12, "Unexpectedly missing documentation files"
    assert (DOCS / "INDEX.md") in documents
    failures: list[str] = []
    examples = {"json": 0, "toml": 0, "mermaid": 0, "bash": 0}
    local_links = 0
    indexed = set()
    index = DOCS / "INDEX.md"

    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    if not re.search(r'<div align="center">\s*<img src="\./assets/full-logo\.png" alt="Jelly" width="760">', readme):
        failures.append("README.md: missing centered 760px logo hero")
    if readme.count('<div align="center">') < 2:
        failures.append("README.md: missing centered hero or demo")
    if '[**Quickstart**](#3-quickstart)' not in readme or '[**Documentation**](#8-documentation-and-support)' not in readme:
        failures.append("README.md: missing original-style navigation")
    if '[documentation index](./docs/INDEX.md)' not in readme or 'assets/porsche-718-spyder-rs-demo-20261008.gif' not in readme:
        failures.append("README.md: missing documentation index or centered demo")
    if "Model Context Protocol (MCP)" not in readme:
        failures.append("README.md: MCP acronym not expanded")
    if "**Browser instrumentation for agents**" not in readme:
        failures.append("README.md: missing official Jelly subtitle")
    for badge in ("Stars", "Forks", "CI", "Rust 1.98.1", "License: Proprietary"):
        if readme.count("[![" + badge + "]") != 1:
            failures.append(f"README.md: badge missing or duplicated: {badge}")
    for document in [ROOT / "README.md", *documents]:
        if "\u2014" in document.read_text(encoding="utf-8"):
            failures.append(f"{document.relative_to(ROOT)}: em dash violates Jelly writing style")
    if "raw_cdp = true" not in (ROOT / "SECURITY.md").read_text(encoding="utf-8"):
        failures.append("SECURITY.md: missing checked-in raw CDP security setting")

    for path in [ROOT / "README.md", *documents]:
        source = path.read_text(encoding="utf-8")
        relative_name = path.relative_to(ROOT)
        if path != ROOT / "README.md":
            if not re.search(r"^# [^#]", source, re.M):
                failures.append(f"{relative_name}: missing document title")
            if path != index and path.name != "GLOSSARY.md" and "Documentation index" not in source:
                failures.append(f"{relative_name}: missing documentation index link")
        for match in LINK.finditer(source):
            ref = (match.group(1) or match.group(2)).strip()
            parsed = urlsplit(ref)
            if parsed.scheme or parsed.netloc:
                continue  # external URL; never fetch credentials or other content
            if not parsed.path and not parsed.fragment:
                continue
            target = (path.parent / unquote(parsed.path)).resolve() if parsed.path else path
            if not target.exists():
                failures.append(f"{relative_name}: missing link target {ref}")
                continue
            local_links += 1
            if parsed.fragment and target.is_file() and unquote(parsed.fragment) not in anchors(target.read_text(encoding="utf-8")):
                failures.append(f"{relative_name}: missing anchor {ref}")
            if path == index and target in documents:
                indexed.add(target)

        for match in BLOCK.finditer(source):
            language, block = match.group(1).strip(), match.group(2)
            if language in {"json", "toml"}:
                try:
                    if language == "json":
                        json.loads(block)
                    else:
                        tomllib.loads(block)
                except (ValueError, json.JSONDecodeError) as exc:
                    failures.append(f"{relative_name}: invalid {language} example: {exc}")
                examples[language] += 1
            elif language == "bash":
                # Syntax-check without running commands or touching runtime state.
                result = subprocess.run(
                    ["bash", "-n", "-c", block],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                if result.returncode:
                    failures.append(f"{relative_name}: invalid Bash snippet: {result.stderr.strip()}")
                examples["bash"] += 1
            elif language == "mermaid":
                first_line = next((line.strip() for line in block.splitlines() if line.strip()), "")
                if not re.match(r"^(flowchart|graph|sequenceDiagram|stateDiagram(?:-v2)?|classDiagram|erDiagram)\b", first_line):
                    failures.append(f"{relative_name}: unsupported Mermaid declaration: {first_line}")
                examples["mermaid"] += 1
    missing = set(documents) - indexed - {index}
    if missing:
        failures.append("INDEX.md does not list: " + ", ".join(str(p.relative_to(DOCS)) for p in sorted(missing)))

    for html_file in [ROOT / "site/index.html", ROOT / "site/404.html"]:
        for match in GITHUB_DOC_URL.finditer(html_file.read_text(encoding="utf-8")):
            target = ROOT / match.group(1)
            if not target.exists():
                failures.append(f"{html_file.relative_to(ROOT)}: broken docs URL: {match.group(0)}")

    technical_config = tomllib.loads((ROOT / "config/jelly.toml").read_text(encoding="utf-8"))
    config_doc = (DOCS / "getting-started/CONFIGURATION.md").read_text(encoding="utf-8")
    tech_example = next(
        (tomllib.loads(block.group(2))
         for block in BLOCK.finditer(config_doc)
         if block.group(1).strip() == "toml" and "[paths]" in block.group(2)),
        None,
    )
    if tech_example is None:
        failures.append("Configuration reference lacks its technical TOML example")
    else:
        for section, fields in tech_example.items():
            if section not in technical_config or not isinstance(fields, dict):
                failures.append(f"Configuration reference section missing in config: {section}")
                continue
            for key, documented in fields.items():
                actual = technical_config[section].get(key)
                if actual != documented:
                    failures.append(
                        f"Config drift: {section}.{key} documented as {documented!r}, "
                        f"checked-in value is {actual!r}"
                    )
    cleanup = (DOCS / "reference/RUNTIME.md").read_text(encoding="utf-8")
    if not all(token in cleanup for token in ("--build", "Cargo build", "OAuth", "extensions")):
        failures.append("Runtime cleanup documentation is missing the destructive side effects")

    print(f"Checked {len(documents)} documents and README.md, {local_links} local links, "
          f"{examples['json']} JSON, {examples['toml']} TOML, and "
          f"{examples['mermaid']} Mermaid blocks, and "
          f"{examples['bash']} Bash syntax checks.")
    if failures:
        for problem in failures:
            print("FAIL:", problem)
        raise SystemExit(1)
    print("PASS: Documentation structure, links, anchors and examples.")


if __name__ == "__main__":
    check()
