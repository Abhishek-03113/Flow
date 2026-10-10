#!/usr/bin/env bash
# Single source of truth for Flow's developer verbs.
# `just <verb>` calls this script; it also works standalone: ./scripts/dev.sh <verb>.
# CI runs the same commands (see .github/workflows/ci.yml).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

UI_DIR="app/ui"
TAURI_DIR="app/src-tauri"

has_ui() { [ -f "$UI_DIR/package.json" ]; }
has_tauri() { [ -d "$TAURI_DIR" ]; }
has_harness() { [ -d harness ] && [ -f harness/Cargo.toml ]; }
ui_script() { grep -q "\"$1\"[[:space:]]*:" "$UI_DIR/package.json"; }
# Run an npm script in app/ui when it exists; skip otherwise.
ui_run() {
  if ! has_ui; then echo "-- skip: $UI_DIR not present ($1)"; return 0; fi
  if ! ui_script "$1"; then echo "-- skip: npm script '$1' not defined"; return 0; fi
  [ -d "$UI_DIR/node_modules" ] || (cd "$UI_DIR" && npm ci)
  (cd "$UI_DIR" && npm run "$1" --silent)
}
step() { printf '\n==> %s\n' "$*"; }

cmd_setup() {
  step "System dependencies"
  case "$(uname -s)" in
    Linux)
      echo "Debian/Ubuntu: sudo apt-get install -y build-essential pkg-config \\"
      echo "  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev"
      echo "Fedora: sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel" ;;
    Darwin)
      echo "macOS: xcode-select --install   (grant Accessibility + Input Monitoring to Flow at runtime)" ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "Windows: install Visual Studio Build Tools (C++ workload) and the WebView2 runtime." ;;
  esac
  step "Rust toolchain"
  rustup show active-toolchain 2>/dev/null || rustc --version
  if has_ui; then
    step "npm ci ($UI_DIR)"
    (cd "$UI_DIR" && npm ci)
    echo "Playwright browsers are NOT installed by setup; run: (cd $UI_DIR && npx playwright install chromium)"
  else
    echo "-- skip: $UI_DIR not present"
  fi
}

cmd_fmt() {
  cargo fmt --all
  if has_ui && ui_script format; then ui_run format; fi
}

cmd_lint() {
  step "cargo fmt --check"
  cargo fmt --all --check
  step "cargo clippy -D warnings"
  cargo clippy --workspace --all-targets -- -D warnings
  step "ui: lint"; ui_run lint
  step "ui: format:check"; ui_run format:check
  step "ui: typecheck"; ui_run typecheck
}

cmd_test() {
  step "cargo test --workspace"
  cargo test --workspace
  step "ui: vitest"; ui_run test
}

cmd_e2e() {
  if ! has_ui; then echo "-- skip: $UI_DIR not present"; return 0; fi
  ui_run e2e
}

cmd_harness() {
  if ! has_harness; then echo "-- skip: harness/ not present"; return 0; fi
  cargo run -p flow-harness --release -- "$@"
}

cmd_dev() {
  has_tauri || { echo "app/src-tauri not present yet" >&2; exit 1; }
  (cd "$UI_DIR" && npx tauri dev "$@")
}

cmd_build() {
  has_tauri || { echo "app/src-tauri not present yet" >&2; exit 1; }
  (cd "$UI_DIR" && npx tauri build "$@")
}

cmd_daemon() { cargo run -p flow-daemon --bin flow-daemon -- "$@"; }

cmd_ci() {
  cmd_lint
  cmd_test
  step "harness"; cmd_harness
  if [ "${SKIP_E2E:-0}" = 1 ]; then echo "-- skip: e2e (SKIP_E2E=1)"; else step "e2e"; cmd_e2e; fi
}

cmd_help() {
  cat <<'USAGE'
Usage: scripts/dev.sh <verb> [args]   (or: just <verb>)
  setup    system-deps hint + npm ci in app/ui
  fmt      cargo fmt + prettier (when app/ui exists)
  lint     fmt --check, clippy -D warnings, eslint/prettier/tsc
  test     cargo test --workspace + vitest
  e2e      Playwright end-to-end tests (app/ui)
  harness  run the flow-harness scenario matrix (if present)
  dev      tauri dev
  build    tauri build
  daemon   run the headless flow-daemon
  ci       everything CI runs (SKIP_E2E=1 to skip Playwright)
USAGE
}

verb="${1:-help}"; shift || true
case "$verb" in
  setup|fmt|lint|test|e2e|harness|dev|build|daemon|ci|help) "cmd_$verb" "$@" ;;
  *) echo "unknown verb: $verb" >&2; cmd_help; exit 2 ;;
esac
