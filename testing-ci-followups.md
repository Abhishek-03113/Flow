# Testing and CI follow-ups

Problems in the test suites and CI that are configuration or tooling, not product bugs. Parked while the
priority is a working v1; pick them up as a separate sprint (like `feature-sprint.md`). Each item says what
was seen and where.

## Speed and signal

### 1. Every CI job rebuilds the whole workspace, including the Tauri app
`rust`, `rust-cross` (macOS + Windows), `harness` and `ui e2e (live engines)` each compile the workspace from
scratch, and `harness` has to install the Tauri/WebKit system packages only because `cargo` builds `flow-app`
with it. Ideas: build `flow-harness` with `-p` only (no WebKit needed), share one cargo cache across jobs
(`Swatinem/rust-cache` keyed by `Cargo.lock`), and run `cargo test --workspace` once on Linux while the
macOS/Windows jobs run `cargo clippy` plus only the `flow-platform` tests (the only OS-specific code).
- Area: `.github/workflows/ci.yml`.

### 2. Every Playwright run rebuilds the UI
`playwright.config.ts` and `playwright.live.config.ts` start `npm run build && vite preview`, so even a single
spec pays for `tsc` + a production build (~20 s locally). Use `vite dev` (or reuse a build when `dist/` is
newer than `src/`) for local runs and keep the production build for CI.
- Area: `app/ui/playwright*.config.ts`.

### 3. The live suite builds the release harness on every run
`e2e-live/global-setup.ts` runs `cargo build -p flow-harness --release` (minutes on a cold cache). A debug build
works for the specs (`FLOW_HARNESS_BIN=target/debug/flow-harness`, used to validate this sprint) and is far
cheaper; make debug the local default.
- Area: `app/ui/e2e-live/global-setup.ts`.

### 4. Screenshot capture is mixed into behaviour tests
Most specs in `e2e/` call `snap()` between steps (each waits for fonts plus 250 ms) and run twice, light and
dark. That doubles the run time of the behaviour checks. Split: behaviour specs once (one theme), a separate
`screens` project that only takes the light/dark screenshots.
- Area: `app/ui/e2e/*.spec.ts`, `app/ui/e2e/helpers.ts`.

### 5. Failures don't say much
The Playwright reporter is `list` only (CI adds HTML only for the live suite); the harness report is uploaded
but not linked from the job summary. Add the HTML reporter + `trace: 'retain-on-failure'` to the mock suite
too, and write the harness summary to `$GITHUB_STEP_SUMMARY`.

## Configuration problems seen

### 6. A Linux test was really a Windows test
`e2e/onboarding.spec.ts` "linux permission guidance" loaded `?os=windows` and asserted the Windows guide, so the
Linux permission screen had no browser test at all. Fixed in this sprint (the new onboarding spec covers Linux,
macOS and Windows), but worth a sweep for similar copy-paste mismatches.

### 7. `tsc -b` from `app/ui` fails; only `npm run typecheck` works
`tsc -b` follows the project references into `design-system/`, which has no `node_modules` of its own, so it
reports hundreds of "Cannot find module 'react'" errors. Running a newer global `tsc` also flags
`baseUrl` in `tsconfig.app.json` as deprecated (TypeScript 6). Either give `design-system` its own install or
drop the reference, and replace `baseUrl` with `paths` relative to the config.
- Area: `app/ui/tsconfig*.json`, `design-system/package.json`.

### 8. jsdom prints "Not implemented: window.open" during unit tests
`src/test/app.test.tsx` exercises `openPermissionSettings`, which calls `window.open` outside Tauri. The tests
pass, but the noise hides real errors. Stub `window.open` in `src/test/setup.ts`.

### 9. Cross-compiling the daemon needs per-target C toolchains
`cargo clippy --target aarch64-apple-darwin|x86_64-pc-windows-msvc` works for `flow-core` and
`flow-platform` but not for `flow-daemon` (`ring` and bundled SQLite compile C). Known and documented in
`daemon/README.md`; CI covers it with native runners. A local `just cross-check` that runs only the two crates
that can cross-compile would make it a habit.

### 10. No test runs on real input devices or real firewalls
Containers have no `/dev/input`, no `uinput` module, no polkit and no firewall, so the Linux one-click setup
was verified by running its script here (it stops at "the uinput kernel module is not available", as it should)
and by `sh -n`, not end to end. macOS permission prompts and the restart flow have only run against the mock.
`docs/testing/physical-test-script.md` is the manual substitute (its Permissions section and the firewall
check in Round 0 cover this sprint's changes); a VM-based Linux job with ufw on would automate part of it.
