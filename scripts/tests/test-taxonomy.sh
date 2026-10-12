#!/usr/bin/env bash
# Offline metadata/classification/acceptance contracts. Never launches Chromium.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
catalog="$(tests/suite/run.sh --catalog --json)"
jq -e '
  . as $catalog |
  (length) as $n |
  ($n > 0) and
  (([.[].id] | unique | length) == $n) and
  all(.[];
    (.id | test("^[A-Z]+-[0-9]{3}$")) and
    (.group | type=="string" and length>0) and
    (.name | type=="string" and length>0) and
    (.description | type=="string" and length>0) and
    (.preconditions | type=="string" and length>0) and
    (.input | type=="string" and length>0) and
    (.expected_output | type=="string" and length>0) and
    (.enabled | type=="boolean") and
    (.scope | IN("unit","component","system","cross-system","not-applicable","unclassified")) and
    (.size | IN("small","medium","large","unclassified")) and
    (.focus | IN("functional","non-functional","tooling","unclassified")) and
    (.execution | IN("automated","manual","hybrid")) and
    (.review_status | IN("provisional","reviewed")) and
    (.environment | type == "string") and
    (.kind | IN("quality","browser","mcp","network")) and
    (.lifecycle | IN("active","deprecated","retired")) and
    (.triage | IN("retain","consolidate","reassess")) and
    (.triage_reason|type=="string" and length>5) and
    (.related_ids|type=="array") and
    (if .lifecycle == "retired" then .review_status == "reviewed" and .enabled == false else true end)
  ) and
  (all(.[]; . as $t | all($t.related_ids[]; . as $ref | $ref != $t.id and any($catalog[]?; .id==$ref))))
' >/dev/null <<< "$catalog" || { echo "Invalid catalog dimensions" >&2; exit 1; }
# Legacy column positions are stable; appended dimensions are queryable.
header="$(tests/suite/run.sh --catalog | sed -n '1p')"
[[ "$header" == $'ID\tGROUP\tNAME\tDESCRIPTION\tPRECONDITIONS\tINPUT\tEXPECTED_OUTPUT\tKIND\tENABLED\tSCOPE\t'* ]] || exit 1
[[ "$(tests/suite/run.sh --catalog --scope cross-system --json | jq length)" -ge 6 ]]
[[ "$(tests/suite/run.sh --catalog --scope system --environment native-browser --json | jq length)" -ge 48 ]]
[[ "$(tests/suite/run.sh --catalog --suite smoke-offline --json | jq length)" -gt 0 ]]
# A batch filter and a suite+group combination have exactly the same selection
# semantics in preview and execution (union of selectors, then dimension filters).
fast_groups="$(awk -F '\t' '$1=="fast" {print $2}' tests/suite/config/batches.tsv)"
IFS=',' read -r -a fast_arr <<< "$fast_groups"
fast_count="$(tests/suite/run.sh --catalog --batch fast --json | jq length)"
[[ "$fast_count" -gt 0 && "$fast_count" -lt "$(jq length <<< "$catalog")" ]]
fast_args=()
for group in "${fast_arr[@]}"; do fast_args+=(--group "$group"); done
[[ "$fast_count" -eq "$(tests/suite/run.sh --catalog "${fast_args[@]}" --json | jq length)" ]]
for batch in deterministic fast; do
  groups="$(awk -F '\t' -v b="$batch" '$1==b{print $2}' tests/suite/config/batches.tsv)"
  [[ ",$groups," != *,quality,* ]] || { echo "Batch $batch duplicates quality gates" >&2; exit 1; }
done
smoke_ids="$(tests/suite/run.sh --catalog --suite smoke-browser --json | jq -r '.[].id' | sort)"
mixed_ids="$(tests/suite/run.sh --catalog --suite smoke-browser --group quality --json | jq -r '.[].id' | sort)"
[[ "$(wc -l <<< "$mixed_ids")" -gt "$(wc -l <<< "$smoke_ids")" ]]
for value in '--scope invalid-scope' '--triage invalid-triage' '--lifecycle unknown'; do
  # SC2086 intentional: split one trusted command argument pair.
  if tests/suite/run.sh --catalog $value --json >/dev/null 2>&1; then
    echo "Unknown selector accepted: $value" >&2; exit 1
  fi
done
# Every suite is a reference set (never another test definition).
while IFS=$'\t' read -r name ids; do
  [[ -z "$name" || "$name" == \#* ]] && continue
  [[ "$name" =~ ^[a-z0-9-]+$ ]] || { echo "Invalid suite name: $name" >&2; exit 1; }
  IFS=',' read -r -a suite_ids <<< "$ids"
  (("${#suite_ids[@]}" > 0)) || { echo "Empty suite $name" >&2; exit 1; }
  for id in "${suite_ids[@]}"; do
    jq -e --arg id "$id" 'any(.[]; .id==$id and .lifecycle!="retired")' >/dev/null <<< "$catalog" || {
      echo "Suite $name references retired/unknown test $id" >&2; exit 1
    }
  done
  [[ "$(printf '%s\n' "${suite_ids[@]}" | sort -u | wc -l)" -eq "${#suite_ids[@]}" ]] || {
    echo "Suite $name contains duplicate IDs" >&2; exit 1;
  }
done < tests/suite/config/suites.tsv
if tests/suite/run.sh --catalog --suite no-such-suite --json >/dev/null 2>&1; then
  echo "Unknown suite was accepted" >&2; exit 1
fi
acceptance="$(bash tests/acceptance/report.sh --json)"
jq -e '
  length >= 6 and
  all(.[]; (.decision | IN("proposed","pending","accepted","rejected")) and
    (.coverage == (if (.evidence_tests | length) > 0 then "linked" else "uncovered" end)))
' >/dev/null <<< "$acceptance" || { echo "Acceptance coverage was misreported" >&2; exit 1; }
bash tests/acceptance/report.sh --validate
echo "PASS: deduplicated ID metadata, lifecycle/triage, consistent selection, suite references and separate acceptance criteria"
