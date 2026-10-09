#!/usr/bin/env bash
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
  setup [--dry-run]           Configure Jelly and optionally install services
  doctor                      Check system prerequisites
  status                      Show Jelly service and MCP status
  install                     Build and install Jelly services
  start                       Start installed Jelly browser and MCP services
  stop                        Stop installed Jelly MCP and browser services
  logs [--follow]             Show recent MCP journal entries or follow them
  clean [--build] [--yes]     Preview or remove runtime and build data
  uninstall [--yes]          Preview or uninstall Jelly MCP services

Development:
  check                       Run project checks
  format [--check]            Format Rust code or check formatting
  lint                        Run Clippy
  test [--isolated|--ranking|--web-ui|--suite [ARGS...]]
                     Run Rust, browser, or integration tests
                     --suite uses the installed browser and services
  build [--release]           Compile Jelly binaries
  tools [--check|--generate]  Verify or regenerate the tool index
  benchmark <surface|browser> --live  Profile the installed browser
  ci                          Run checks and Rust tests
  help                        Show command help
USAGE
}

cmd="${1:-help}"
if (($#)); then shift; fi
case "$cmd" in
  help|-h|--help) (($# == 0)) || { dev::error 'help takes no arguments'; exit 2; }; usage ;;
  setup) exec "$DEV_ROOT/scripts/commands/setup.sh" "$@" ;;
  doctor)
    (($# == 0)) || { dev::error 'doctor takes no arguments'; exit 2; }
    dev::doctor ;;
  status|install)
    (($# == 0)) || { dev::error "$cmd takes no arguments"; exit 2; }
    if [[ "$cmd" == status ]]; then
      exec "$DEV_ROOT/scripts/commands/status.sh"
    fi
    exec "$DEV_ROOT/scripts/commands/install.sh" ;;
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
  uninstall)
    if (($# == 0)); then
      dev::warning 'Preview only: uninstall would disable/stop the Jelly MCP and Cloudflare user units and remove their installed unit files.'
      dev::warning 'Run uninstall --yes to perform this operation.'
      exit 0
    fi
    [[ "$#" == 1 && "$1" == --yes ]] || { dev::error 'usage: dev.sh uninstall [--yes]'; exit 2; }
    exec "$DEV_ROOT/scripts/commands/uninstall.sh" ;;
  clean)
    build=false yes=false
    for arg in "$@"; do
      case "$arg" in --build) build=true ;; --yes) yes=true ;; *) dev::error "invalid clean argument: $arg"; exit 2 ;; esac
    done
    dev::ui_init
    # Sourcing technical configuration is read-only and does not compile or load .env.
    # shellcheck source=config.sh
    source "$DEV_ROOT/scripts/lib/config.sh"
    dev::warning 'clean stops installed Jelly services and deletes configured runtime/build state.'
    printf '  Runtime directory: %s\n' "$CONFIG_RUNTIME_ROOT"
    printf '  Cargo build directory: %s\n' "$CONFIG_BUILD_ROOT"
    printf '  Action: scripts/commands/clean.sh%s\n' "$( [[ "$build" == true ]] && printf ' --build' || true )"
    if [[ "$yes" != true ]]; then
      dev::warning 'Preview only. Use clean --yes to execute the destructive operation.'
      exit 0
    fi
    if [[ "$build" == true ]]; then
      exec "$DEV_ROOT/scripts/commands/clean.sh" --build
    fi
    exec "$DEV_ROOT/scripts/commands/clean.sh" ;;
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
    "$DEV_ROOT/scripts/dev.sh" tools --check
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
      --isolated) (($# == 1)) || { dev::error 'test --isolated takes no other arguments'; exit 2; }; exec "$DEV_ROOT/scripts/tests/fcis-isolated.sh" ;;
      --web-ui) (($# == 1)) || { dev::error 'test --web-ui takes no other arguments'; exit 2; }; exec "$DEV_ROOT/scripts/tests/web-ui.sh" ;;
      --suite)
        shift
        case "${1:-}" in
          --list|--catalog|--help|-h) ;;
          *) dev::warning 'The installed Jelly browser/service may be affected by this test suite.' ;;
        esac
        exec "$DEV_ROOT/tests/suite/run.sh" "$@" ;;
      *) dev::error 'usage: dev.sh test [--ranking|--isolated|--web-ui|--suite [ARGS...]]'; exit 2 ;;
    esac ;;
  build)
    dev::require cargo
    cd "$DEV_ROOT"
    case "${1:-}" in
      '') cargo build --locked --bins ;;
      --release) (($# == 1)) || { dev::error 'build --release takes no other arguments'; exit 2; }; cargo build --locked --release --bins ;;
      *) dev::error 'usage: dev.sh build [--release]'; exit 2 ;;
    esac ;;
  tools)
    dev::require cargo
    cd "$DEV_ROOT"
    case "${1:---check}" in
      --check) (($# <= 1)) || { dev::error 'tools --check takes no extra arguments'; exit 2; }; cargo run --quiet --locked --bin build-tool-index -- --check ;;
      --generate) (($# == 1)) || { dev::error 'tools --generate takes no extra arguments'; exit 2; }; cargo run --quiet --locked --bin build-tool-index ;;
      *) dev::error 'usage: dev.sh tools [--check|--generate]'; exit 2 ;;
    esac ;;
  benchmark)
    [[ "$#" == 2 && "$2" == --live ]] || {
      dev::error 'benchmark uses the installed Jelly browser; specify benchmark <surface|browser> --live explicitly'
      exit 2
    }
    case "$1" in
      surface) exec "$DEV_ROOT/scripts/diagnostics/mcp-surface.sh" ;;
      browser) exec "$DEV_ROOT/scripts/diagnostics/browser-runtime.sh" ;;
      *) dev::error 'usage: dev.sh benchmark <surface|browser> --live'; exit 2 ;;
    esac ;;
  ci)
    (($# == 0)) || { dev::error 'ci takes no arguments'; exit 2; }
    "$DEV_ROOT/scripts/dev.sh" check
    "$DEV_ROOT/scripts/dev.sh" test ;;
  *) dev::error "unknown command '$cmd'; run ./scripts/dev.sh --help"; exit 2 ;;
esac
