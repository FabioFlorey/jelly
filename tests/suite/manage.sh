#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
TESTS_FILE="tests/suite/config/disabled-tests.txt"
GROUPS_FILE="tests/suite/config/disabled-groups.txt"

usage() {
  cat <<'EOF'
Usage:
  tests/suite/manage.sh status
  tests/suite/manage.sh disable-test TEST-ID
  tests/suite/manage.sh enable-test TEST-ID
  tests/suite/manage.sh disable-group GROUP
  tests/suite/manage.sh enable-group GROUP
EOF
}

clean_values() {
  sed -e 's/[[:space:]]*#.*$//' -e '/^[[:space:]]*$/d' "$1" 2>/dev/null || true
}

write_values() {
  local file="$1" header="$2"
  shift 2
  {
    echo "# $header"
    printf '%s\n' "$@" | sed '/^$/d' | sort -u
  } > "$file"
}

valid_test() {
  tests/suite/run.sh --list | awk -F '\t' -v id="$1" 'NR>1 && $1==id{found=1} END{exit !found}'
}

valid_group() {
  tests/suite/run.sh --list | awk -F '\t' -v g="$1" 'NR>1 && $2==g{found=1} END{exit !found}'
}

toggle_value() {
  local action="$1" file="$2" value="$3" header="$4"
  mapfile -t current < <(clean_values "$file")
  local -a next=()
  local item found=false
  for item in "${current[@]}"; do
    if [[ "$item" == "$value" ]]; then
      found=true
      [[ "$action" == disable ]] && next+=("$item")
    else
      next+=("$item")
    fi
  done
  if [[ "$action" == disable && "$found" == false ]]; then
    next+=("$value")
  fi
  write_values "$file" "$header" "${next[@]}"
}

cmd="${1:-status}"
case "$cmd" in
  status)
    echo 'Disabled tests:'
    clean_values "$TESTS_FILE" | sed 's/^/  /' || true
    echo 'Disabled groups:'
    clean_values "$GROUPS_FILE" | sed 's/^/  /' || true
    echo
    tests/suite/run.sh --list
    ;;
  disable-test|enable-test)
    id="${2:?missing test ID}"
    valid_test "$id" || { echo "unknown test ID: $id" >&2; exit 2; }
    action="${cmd%-test}"
    toggle_value "$action" "$TESTS_FILE" "$id" 'One stable test ID per line. Empty means no individually disabled tests.'
    ;;
  disable-group|enable-group)
    group="${2:?missing group name}"
    valid_group "$group" || { echo "unknown group: $group" >&2; exit 2; }
    action="${cmd%-group}"
    toggle_value "$action" "$GROUPS_FILE" "$group" 'One group name per line. Empty means no disabled groups.'
    ;;
  *) usage >&2; exit 2 ;;
esac
