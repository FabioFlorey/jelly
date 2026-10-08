#!/usr/bin/env bash

# Shared Jelly configuration for shell tooling.
# Technical settings come only from config/jelly.toml.
# .env is reserved for secrets and deployment-specific environment values.

: "${REPO_ROOT:=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
CONFIG_FILE="$REPO_ROOT/config/jelly.toml"
CARGO_CONFIG_FILE="$REPO_ROOT/config/cargo.toml"

# Keep caller-supplied environment values authoritative over .env secrets/deployment values.
declare -A _JELLY_ENV_OVERRIDES=()
while IFS= read -r _jelly_name; do
  [[ -n "$_jelly_name" ]] || continue
  _JELLY_ENV_OVERRIDES["$_jelly_name"]="${!_jelly_name}"
done < <(compgen -A variable JELLY_)

if [[ -f "$REPO_ROOT/.env" ]]; then
  _jelly_rows="$(python3 "$REPO_ROOT/scripts/env-data.py" read "$REPO_ROOT/.env")" || return 2 2>/dev/null || exit 2
  while IFS=$'\t' read -r _jelly_name _jelly_encoded; do
    [[ -n "$_jelly_name" ]] || continue
    _jelly_value="$(printf '%s' "$_jelly_encoded" | base64 --decode)" || return 2 2>/dev/null || exit 2
    printf -v "$_jelly_name" '%s' "$_jelly_value"
    export "$_jelly_name"
  done <<< "$_jelly_rows"
  unset _jelly_rows _jelly_encoded _jelly_value
fi

for _jelly_name in "${!_JELLY_ENV_OVERRIDES[@]}"; do
  printf -v "$_jelly_name" '%s' "${_JELLY_ENV_OVERRIDES[$_jelly_name]}"
  export "$_jelly_name"
done
unset _jelly_name _JELLY_ENV_OVERRIDES

jelly_config_set() {
  local section="$1" key="$2" value="$3"
  python3 - "$CONFIG_FILE" "$section" "$key" "$value" <<'PY'
from pathlib import Path
import json, re, sys
path, section, key, value = Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
lines = path.read_text().splitlines()
if value in {"true", "false"} or re.fullmatch(r"-?\d+", value):
    rendered = value
else:
    rendered = json.dumps(value)
section_line = f"[{section}]"
inside = False
found_section = False
for index, line in enumerate(lines):
    stripped = line.strip()
    if stripped.startswith("[") and stripped.endswith("]"):
        inside = stripped == section_line
        found_section = found_section or inside
        continue
    if inside and re.match(rf"^\s*{re.escape(key)}\s*=", line):
        lines[index] = f"{key} = {rendered}"
        path.write_text("\n".join(lines) + "\n")
        break
else:
    if not found_section:
        raise SystemExit(f"missing [{section}] in {path}")
    raise SystemExit(f"missing {section}.{key} in {path}")
PY
}

config_value() {
  local file="$1" section="$2" key="$3"
  [[ -f "$file" ]] || return 1
  awk -v section="[$section]" -v key="$key" '
    $0 == section { inside=1; next }
    /^\[/ { inside=0 }
    inside && $0 ~ "^[[:space:]]*" key "[[:space:]]*=" {
      sub("^[[:space:]]*" key "[[:space:]]*=[[:space:]]*", "")
      sub(/[[:space:]]*#.*/, "")
      gsub(/^[[:space:]]+|[[:space:]]+$/, "")
      gsub(/^"|"$/, "")
      print
      exit
    }
  ' "$file"
}

jelly_config_value() { config_value "$CONFIG_FILE" "$@"; }
cargo_config_value() { config_value "$CARGO_CONFIG_FILE" "$@"; }

CONFIG_RUNTIME_ROOT="$(jelly_config_value paths runtime_root)"
CONFIG_BUILD_ROOT="$(cargo_config_value build target-dir)"
CONFIG_TEST_DEFAULT_BATCH="$(jelly_config_value tests default_batch)"
CONFIG_UI_ICONS="$(jelly_config_value ui icons)"
CONFIG_UI_ANIMATION="$(jelly_config_value ui animation)"
CONFIG_UI_LOGO="$(jelly_config_value ui logo)"

[[ -n "$CONFIG_RUNTIME_ROOT" ]] || { echo "missing paths.runtime_root in $CONFIG_FILE" >&2; return 2 2>/dev/null || exit 2; }
[[ -n "$CONFIG_BUILD_ROOT" ]] || { echo "missing build.target-dir in $CARGO_CONFIG_FILE" >&2; return 2 2>/dev/null || exit 2; }
[[ -n "$CONFIG_TEST_DEFAULT_BATCH" ]] || { echo "missing tests.default_batch in $CONFIG_FILE" >&2; return 2 2>/dev/null || exit 2; }
[[ -n "$CONFIG_UI_ICONS" ]] || { echo "missing ui.icons in $CONFIG_FILE" >&2; return 2 2>/dev/null || exit 2; }
[[ -n "$CONFIG_UI_ANIMATION" ]] || { echo "missing ui.animation in $CONFIG_FILE" >&2; return 2 2>/dev/null || exit 2; }
[[ -n "$CONFIG_UI_LOGO" ]] || { echo "missing ui.logo in $CONFIG_FILE" >&2; return 2 2>/dev/null || exit 2; }
