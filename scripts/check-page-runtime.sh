#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
exec tests/suite/run.sh --group runtime "$@"
