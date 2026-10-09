#!/usr/bin/env bash
# Jelly's single documented CLI: setup, operations, and developer workflows.
set -Eeuo pipefail
DEV_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=lib/runtime.sh
source "$DEV_ROOT/scripts/lib/runtime.sh"
dev::ui_init

usage() {
  cat <<'USAGE'
Jelly CLI
Usage: ./scripts/dev.sh <command> [arguments]

Installation and operations:
  setup [--dry-run]   Complete interactive setup, including hosting, OAuth and optional installation
  doctor              Read-only prerequisite audit; never builds or reads .env
  status              Read-only service and MCP status
  install             Build/install/update Jelly user services (explicitly state-changing)
  start               Start installed Jelly browser and MCP services
  stop                Stop installed Jelly MCP and browser services
  logs [--follow]     Show recent MCP journal entries or follow them
  clean [--build] [--yes]  Preview destructive cleanup; execute only with --yes

Development:
  check               Format, lint, docs, security, architecture and guidance checks
  format [--check]    Format Rust code, or only verify formatting
  lint                Run Rust Clippy with warnings denied
  test [--isolated|--ranking|--suite [ARGS...]]
                     Default: offline Rust tests; --isolated uses disposable Chromium/MCP
                     --suite explicitly targets the installed browser/service (use with care)
  build [--release]   Compile Jelly binaries
  ci                  Run local static checks and offline Rust tests
  help                Show this message

Compatibility: setup --check acts like doctor; setup --dry-run never installs or writes secrets.
No Python, Node.js, Gum, Bats, ShellCheck, or shfmt required for normal use.
USAGE
}

cmd="${1:-help}"
if (($#)); then shift; fi
case "$cmd" in
  help|-h|--help) (($# == 0)) || { dev::error 'help takes no arguments'; exit 2; }; usage ;;
  setup)
    if [[ "${1:-}" == --check && $# == 1 ]]; then
      dev::doctor
    else
      exec "$DEV_ROOT/scripts/commands/setup.sh" "$@"
    fi
    ;;
  doctor)
    (($# == 0)) || { dev::error 'doctor takes no arguments'; exit 2; }
    dev::doctor ;;
  status|install)
    (($# == 0)) || { dev::error "$cmd takes no arguments"; exit 2; }
    if [[ "$cmd" == status ]]; then
      exec "$DEV_ROOT/scripts/status-mcp-services.sh"
    fi
    exec "$DEV_ROOT/scripts/install-mcp-services.sh" ;;
  start|stop)
    (($# == 0)) || { dev::error "$cmd takes no arguments"; exit 2; }
    dev::require systemctl
    if [[ "$cmd" == start ]]; then
      systemctl --user start jelly-browser.service
      systemctl --user start jelly-mcp.service
    else
      systemctl --user stop jelly-mcp.service
      systemctl --user stop jelly-browser.service
    fi
    ;;
  logs)
    dev::require journalctl
    case "${1:-}" in
      '') journalctl --user -u jelly-mcp.service -n 80 --no-pager ;;
      --follow) (($# == 1)) || { dev::error 'logs --follow takes no other arguments'; exit 2; }; journalctl --user -u jelly-mcp.service -f ;;
      *) dev::error 'usage: dev.sh logs [--follow]'; exit 2 ;;
    esac ;;
  clean)
    build=false yes=false
    for arg in "$@"; do
      case "$arg" in --build) build=true ;; --yes) yes=true ;; *) dev::error "invalid clean argument: $arg"; exit 2 ;; esac
    done
    dev::ui_init
    # Sourcing technical configuration is read-only and does not compile or load .env.
    # shellcheck source=config.sh
    source "$DEV_ROOT/scripts/config.sh"
    dev::warning 'clean stops installed Jelly services and deletes configured runtime/build state.'
    printf '  Runtime directory: %s\n' "$CONFIG_RUNTIME_ROOT"
    printf '  Cargo build directory: %s\n' "$CONFIG_BUILD_ROOT"
    printf '  Action: scripts/clean-runtime.sh%s\n' "$( [[ "$build" == true ]] && printf ' --build' || true )"
    if [[ "$yes" != true ]]; then
      dev::warning 'Preview only. Use clean --yes to execute the destructive operation.'
      exit 0
    fi
    if [[ "$build" == true ]]; then
      exec "$DEV_ROOT/scripts/clean-runtime.sh" --build
    fi
    exec "$DEV_ROOT/scripts/clean-runtime.sh" ;;
  check)
    (($# == 0)) || { dev::error 'check takes no arguments'; exit 2; }
    cd "$DEV_ROOT"
    dev::require cargo
    cargo fmt --all -- --check
    cargo clippy --all-targets --locked -- -D warnings
    for kind in docs security architecture agent-guidance; do
      if [[ "$kind" == agent-guidance ]]; then
        cargo run --quiet --locked --bin jelly-maint -- check agent-guidance --self-test
      else
        cargo run --quiet --locked --bin jelly-maint -- check "$kind"
      fi
    done
    "$DEV_ROOT/scripts/tests/dev-cli.sh" ;;
  format)
    dev::require cargo
    cd "$DEV_ROOT"
    case "${1:-}" in
      '') cargo fmt --all ;;
      --check) (($# == 1)) || { dev::error 'format --check takes no other arguments'; exit 2; }; cargo fmt --all -- --check ;;
      *) dev::error 'usage: dev.sh format [--check]'; exit 2 ;;
    esac ;;
  lint)
    (($# == 0)) || { dev::error 'lint takes no arguments'; exit 2; }
    dev::require cargo
    cd "$DEV_ROOT"
    cargo clippy --all-targets --locked -- -D warnings ;;
  test)
    dev::require cargo
    cd "$DEV_ROOT"
    case "${1:-}" in
      '') cargo test --lib --locked ;;
      --ranking) (($# == 1)) || { dev::error 'test --ranking takes no other arguments'; exit 2; }; cargo run --locked --bin jelly-maint -- test ranking ;;
      --isolated) (($# == 1)) || { dev::error 'test --isolated takes no other arguments'; exit 2; }; exec "$DEV_ROOT/scripts/check-fcis-isolated.sh" ;;
      --suite)
        shift
        case "${1:-}" in
          --list|--catalog|--help|-h) ;;
          *) dev::warning 'The installed Jelly browser/service may be affected by this test suite.' ;;
        esac
        exec "$DEV_ROOT/tests/suite/run.sh" "$@" ;;
      *) dev::error 'usage: dev.sh test [--ranking|--isolated|--suite [ARGS...]]'; exit 2 ;;
    esac ;;
  build)
    dev::require cargo
    cd "$DEV_ROOT"
    case "${1:-}" in
      '') cargo build --locked --bins ;;
      --release) (($# == 1)) || { dev::error 'build --release takes no other arguments'; exit 2; }; cargo build --locked --release --bins ;;
      *) dev::error 'usage: dev.sh build [--release]'; exit 2 ;;
    esac ;;
  ci)
    (($# == 0)) || { dev::error 'ci takes no arguments'; exit 2; }
    "$DEV_ROOT/scripts/dev.sh" check
    "$DEV_ROOT/scripts/dev.sh" test ;;
  *) dev::error "unknown command '$cmd'; run ./scripts/dev.sh --help"; exit 2 ;;
esac
