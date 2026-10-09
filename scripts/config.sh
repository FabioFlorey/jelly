#!/usr/bin/env bash

# Shared Jelly configuration for shell tooling.
# Technical settings come only from config/jelly.toml.
# .env is reserved for secrets and deployment-specific environment values.

: "${REPO_ROOT:=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
CONFIG_FILE="$REPO_ROOT/config/jelly.toml"
CARGO_CONFIG_FILE="$REPO_ROOT/config/cargo.toml"

# A sourced library must be inert: callers load .env explicitly when needed.
# The Rust helper treats entries strictly as data, never as shell expressions.
jelly_maint() {
  cargo run --quiet --locked --manifest-path "$REPO_ROOT/Cargo.toml" --bin jelly-maint -- "$@"
}

jelly_load_env() {
  [[ -f "$REPO_ROOT/.env" ]] || return 0
  local _jelly_rows _jelly_name _jelly_encoded _jelly_value
  local -A _jelly_overrides=()
  while IFS= read -r _jelly_name; do
    [[ -n "$_jelly_name" ]] || continue
    _jelly_overrides["$_jelly_name"]="${!_jelly_name}"
  done < <(compgen -A variable JELLY_)
  if [[ "${1:-}" == --existing-bin ]]; then
    local existing_bin="$CONFIG_BUILD_ROOT/debug/jelly-maint"
    if [[ ! -x "$existing_bin" ]]; then
      existing_bin="$CONFIG_BUILD_ROOT/release/jelly-maint"
    fi
    if [[ ! -x "$existing_bin" ]]; then
      echo 'Jelly status: no built maintenance parser; .env values not loaded (no build performed).' >&2
      return 0
    fi
    _jelly_rows="$("$existing_bin" env read "$REPO_ROOT/.env")" || return 2
  elif (($# == 0)); then
    _jelly_rows="$(jelly_maint env read "$REPO_ROOT/.env")" || return 2
  else
    echo 'usage: jelly_load_env [--existing-bin]' >&2
    return 2
  fi
  while IFS=$'\t' read -r _jelly_name _jelly_encoded; do
    [[ -n "$_jelly_name" ]] || continue
    _jelly_value="$(printf '%s' "$_jelly_encoded" | base64 --decode)" || return 2
    printf -v "$_jelly_name" '%s' "$_jelly_value"
    export "$_jelly_name"
  done <<< "$_jelly_rows"
  for _jelly_name in "${!_jelly_overrides[@]}"; do
    printf -v "$_jelly_name" '%s' "${_jelly_overrides[$_jelly_name]}"
    export "$_jelly_name"
  done
}

jelly_config_set() {
  local section="$1" key="$2" value="$3"
  jelly_maint config set "$CONFIG_FILE" "$section" "$key" "$value"
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
