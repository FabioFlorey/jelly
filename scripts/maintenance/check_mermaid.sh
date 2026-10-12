#!/usr/bin/env bash
# Render tracked Markdown Mermaid blocks using the Mermaid CLI.
# Usage: check_mermaid.sh /path/to/mmdc
set -Eeuo pipefail

if [[ $# -ne 1 || ! -x "$1" ]]; then
  printf 'usage: %s /path/to/mmdc\n' "$0" >&2
  exit 2
fi

mmdc="$(realpath "$1")"
root="$(cd "$(dirname "$0")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/jelly-mermaid-check.XXXXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT
cd "$root"

checked=0
failed=0
document=0

while IFS= read -r -d '' path; do
  document=$((document + 1))
  manifest="$scratch/manifest"
  # Extract only complete Mermaid fences. Each chart gets its own file so
  # errors identify the original Markdown document and its chart number.
  if ! awk -v root="$scratch" -v doc="$document" '
    /^[[:space:]]*```mermaid[[:space:]]*$/ {
      if (inside) exit 2
      inside = 1
      index_in_doc++
      output = root "/document-" doc "-chart-" index_in_doc ".mmd"
      next
    }
    inside && /^[[:space:]]*```[[:space:]]*$/ {
      close(output)
      print output
      inside = 0
      next
    }
    inside { print > output }
    END { if (inside) exit 2 }
  ' "$path" > "$manifest"; then
    printf 'FAIL %s: invalid Mermaid fence\n' "$path" >&2
    failed=$((failed + 1))
    continue
  fi

  chart=0
  while IFS= read -r input; do
    chart=$((chart + 1))
    checked=$((checked + 1))
    output="${input%.mmd}.svg"
    log="${input%.mmd}.log"
    if "$mmdc" -i "$input" -o "$output" > "$log" 2>&1 && [[ -s "$output" ]]; then
      printf 'PASS %s chart %d\n' "$path" "$chart"
    else
      printf 'FAIL %s chart %d\n' "$path" "$chart" >&2
      cat -- "$log" >&2
      failed=$((failed + 1))
    fi
  done < "$manifest"
done < <(git ls-files -z -- '*.md')

if (( checked == 0 )); then
  printf 'FAIL: no Mermaid diagrams found\n' >&2
  exit 1
fi
printf 'Rendered %d Mermaid diagrams: %d failed.\n' "$checked" "$failed"
(( failed == 0 ))
