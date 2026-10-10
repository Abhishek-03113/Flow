# Flow task runner. Every recipe delegates to scripts/dev.sh, so
# `./scripts/dev.sh <verb>` works identically without `just` installed.

set shell := ["bash", "-euo", "pipefail", "-c"]

# list recipes
default:
    @just --list

# system deps hint + npm ci in app/ui
setup:
    ./scripts/dev.sh setup

# format Rust (and UI when present)
fmt:
    ./scripts/dev.sh fmt

# fmt --check, clippy -D warnings, eslint/prettier/tsc
lint:
    ./scripts/dev.sh lint

# cargo test --workspace + vitest
test:
    ./scripts/dev.sh test

# Playwright end-to-end tests
e2e:
    ./scripts/dev.sh e2e

# run the flow-harness scenario matrix
harness *ARGS:
    ./scripts/dev.sh harness {{ARGS}}

# tauri dev
dev *ARGS:
    ./scripts/dev.sh dev {{ARGS}}

# tauri build (release bundles)
build *ARGS:
    ./scripts/dev.sh build {{ARGS}}

# run the headless daemon
daemon *ARGS:
    ./scripts/dev.sh daemon {{ARGS}}

# everything CI runs, locally (SKIP_E2E=1 to skip Playwright)
ci:
    ./scripts/dev.sh ci
