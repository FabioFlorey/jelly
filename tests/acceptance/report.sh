#!/usr/bin/env bash
# Requirement-to-test traceability. A passing test is evidence, never a UAT approval.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
format=table
case "${1:-}" in
  '') ;;
  --json) format=json; shift ;;
  --validate) format=validate; shift ;;
  *) echo 'usage: tests/acceptance/report.sh [--json|--validate]' >&2; exit 2 ;;
esac
(($# == 0)) || { echo 'Unexpected arguments' >&2; exit 2; }
command -v jq >/dev/null || { echo 'jq required' >&2; exit 2; }
criteria_file=tests/acceptance/criteria.tsv
links_file=tests/acceptance/links.tsv
criteria_json="$(mktemp)"
links_json="$(mktemp)"
trap 'rm -f -- "$criteria_json" "$links_json"' EXIT
printf '[]\n' > "$criteria_json"
printf '[]\n' > "$links_json"

declare -A known_criteria=()
while IFS=$'\t' read -r id title statement owner decision; do
  [[ -z "$id" || "$id" == \#* ]] && continue
  [[ "$id" =~ ^AC-[A-Z]+-[0-9]{3}$ ]] || { echo "Invalid acceptance ID: $id" >&2; exit 2; }
  [[ -z "${known_criteria[$id]:-}" ]] || { echo "Duplicate criterion: $id" >&2; exit 2; }
  [[ -n "$title" && -n "$statement" && -n "$owner" ]] || { echo "Incomplete criterion: $id" >&2; exit 2; }
  case "$decision" in proposed|pending|accepted|rejected) ;; *) echo "Invalid decision: $decision" >&2; exit 2 ;; esac
  known_criteria["$id"]=1
  row="$(jq -cn --arg id "$id" --arg title "$title" --arg statement "$statement" --arg owner "$owner" --arg decision "$decision" '{id:$id,title:$title,statement:$statement,owner:$owner,decision:$decision}')"
  jq --argjson row "$row" '. + [$row]' "$criteria_json" > "$criteria_json.next"
  mv "$criteria_json.next" "$criteria_json"
done < "$criteria_file"

catalog="$(tests/suite/run.sh --catalog --json)"
declare -A linked_pair=()
while IFS=$'\t' read -r criterion test_id role; do
  [[ -z "$criterion" || "$criterion" == \#* ]] && continue
  [[ -n "${known_criteria[$criterion]:-}" ]] || { echo "Unknown acceptance criterion: $criterion" >&2; exit 2; }
  if ! jq -e --arg id "$test_id" 'any(.[]; .id==$id and .lifecycle!="retired")' >/dev/null <<< "$catalog"; then
    echo "Unknown test ID in acceptance mapping: $test_id" >&2; exit 2
  fi
  case "$role" in primary|supporting) ;; *) echo "Invalid evidence role: $role" >&2; exit 2 ;; esac
  key="$criterion:$test_id"
  [[ -z "${linked_pair[$key]:-}" ]] || { echo "Duplicate acceptance link: $key" >&2; exit 2; }
  linked_pair["$key"]=1
  row="$(jq -cn --arg criterion "$criterion" --arg test_id "$test_id" --arg role "$role" '{criterion_id:$criterion,test_id:$test_id,role:$role}')"
  jq --argjson row "$row" '. + [$row]' "$links_json" > "$links_json.next"
  mv "$links_json.next" "$links_json"
done < "$links_file"

report="$(jq -n --slurpfile criteria "$criteria_json" --slurpfile links "$links_json" '
  [$criteria[0][] | . as $c |
    ($links[0] | map(select(.criterion_id == $c.id) | {test_id,role})) as $e |
    ($c + {evidence_tests:$e,coverage:(if ($e|length)>0 then "linked" else "uncovered" end)})]
')"
case "$format" in
  json) printf '%s\n' "$report" ;;
  table)
    printf 'CRITERION\tDECISION\tCOVERAGE\tEVIDENCE_TESTS\tTITLE\n'
    jq -r '.[]|[.id,.decision,.coverage,([.evidence_tests[].test_id]|join(",")),.title]|@tsv' <<< "$report" ;;
  validate)
    printf 'PASS: %s acceptance criteria, %s evidence links; uncovered criteria are tracked without implying approval\n' \
      "$(jq length <<< "$report")" "$(jq '[.[].evidence_tests|length]|add//0' <<<"$report")" ;;
esac
