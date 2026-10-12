#!/usr/bin/env python3
"""Render every tracked Markdown Mermaid chart with the Mermaid CLI.

Usage: check_mermaid.py /path/to/mmdc
The Mermaid CLI requires a Chromium executable (PUPPETEER_EXECUTABLE_PATH).
This check complements the offline Rust documentation link/example checker.
"""

import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FENCE = chr(96) * 3
BLOCK = re.compile(
    r"^" + re.escape(FENCE) + r"mermaid[ \t]*\r?\n(.*?)^" + re.escape(FENCE) + r"[ \t]*$",
    re.MULTILINE | re.DOTALL,
)


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check_mermaid.py /path/to/mmdc", file=sys.stderr)
        return 2
    mmdc = Path(sys.argv[1]).resolve()
    if not mmdc.is_file():
        print(f"Mermaid CLI not found: {mmdc}", file=sys.stderr)
        return 2

    files = subprocess.check_output(
        ["git", "ls-files", "-z", "--", "*.md"], cwd=ROOT
    ).decode().split("\0")
    count = 0
    errors = 0
    with tempfile.TemporaryDirectory(prefix="jelly-mermaid-check-") as directory:
        temp = Path(directory)
        for name in filter(None, files):
            text = (ROOT / name).read_text()
            for index, match in enumerate(BLOCK.finditer(text), 1):
                count += 1
                line = text.count("\n", 0, match.start()) + 1
                source = temp / f"chart-{count}.mmd"
                output = temp / f"chart-{count}.svg"
                source.write_text(match.group(1) + "\n")
                try:
                    result = subprocess.run(
                        [str(mmdc), "-i", str(source), "-o", str(output)],
                        capture_output=True,
                        text=True,
                        timeout=45,
                        check=False,
                    )
                except subprocess.TimeoutExpired:
                    errors += 1
                    print(f"FAIL {name}:{line} chart {index}: renderer timed out")
                    continue
                if result.returncode != 0 or not output.is_file():
                    errors += 1
                    details = (result.stderr or result.stdout).strip()
                    print(f"FAIL {name}:{line} chart {index}:")
                    print(details[:1800])
                else:
                    print(f"PASS {name}:{line} chart {index}")
    if count == 0:
        print("FAIL: no Mermaid diagrams found", file=sys.stderr)
        return 1
    print(f"Rendered {count} Mermaid diagrams: {errors} failed.")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
