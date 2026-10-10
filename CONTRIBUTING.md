# Contributing to Flow

Thanks for helping! Flow is a software KVM: one keyboard and mouse controlling several computers. The design lives in
[`docs/redesign/PLAN.md`](docs/redesign/PLAN.md) and [`docs/adr/`](docs/adr/); read the plan's "Engineering rules" first.

## Repository layout

```
core/            flow-core       protocol v2, input model, keymap model, settings/layout model (no OS deps)
platform/        flow-platform   per-OS capture/injection + keymap loading
daemon/          flow-daemon     the engine as a library + a headless `flow-daemon` binary
harness/         flow-harness    virtual platform + multi-daemon scenario runner, CLI and HTML report
app/             Tauri 2 desktop app
  src-tauri/     flow-app        embeds the engine, tray, windows, Tauri commands/events
  ui/            React + TypeScript + Vite frontend, Vitest + Playwright
config/keymaps/  default per-OS keymaps (windows/macos/linux .toml), embedded at compile time
docs/            adr/, redesign/, contracts/, progress/ (milestone screenshots)
scripts/         dev.sh, the implementation behind the justfile
```

Some directories (`app/`, `harness/`) land incrementally during the 2.0 redesign; the task runner and CI skip what
is not present yet.

## Setup

1. Install Rust via [rustup](https://rustup.rs). `rust-toolchain.toml` pins the version and components
   (rustfmt, clippy) and rustup installs them automatically.
2. Install Node 22 (only needed for `app/ui`).
3. Optionally install [`just`](https://github.com/casey/just) (`cargo install just`, `brew install just`,
   `winget install Casey.Just`). Without it, use `./scripts/dev.sh <verb>`, which has the same verbs.
4. System dependencies:
   - **Linux (Debian/Ubuntu):** `sudo apt-get install -y build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`.
     Real input capture needs read access to `/dev/input` and write access to `/dev/uinput`.
   - **macOS:** `xcode-select --install`. At runtime grant Accessibility and Input Monitoring to the app.
   - **Windows:** Visual Studio Build Tools (C++ workload) and the WebView2 runtime.
5. Run `just setup` (prints the hints above and runs `npm ci` in `app/ui`).

## Task runner

| Verb | What it does |
|---|---|
| `just setup` | system-deps hint, `npm ci` in `app/ui` |
| `just fmt` | `cargo fmt` (+ prettier when present) |
| `just lint` | `cargo fmt --check`, `clippy -D warnings`, eslint, prettier, `tsc` |
| `just test` | `cargo test --workspace` + Vitest |
| `just e2e` | Playwright end-to-end tests against the UI |
| `just harness` | run the multi-daemon scenario matrix and write the report |
| `just dev` | `tauri dev` |
| `just build` | `tauri build` (installers) |
| `just daemon` | run the headless daemon |
| `just ci` | everything CI runs (`SKIP_E2E=1 just ci` skips Playwright) |

## Branches and commits

Branch names are `<type>/<scope>-<desc>`, for example `feat/engine-controller-model`, `fix/platform-macos-modifiers`,
`ci/platform-engineering`. Types: `feat`, `fix`, `refactor`, `test`, `chore`, `docs`, `ci`.

Commits follow [Conventional Commits](https://www.conventionalcommits.org): `<type>(<scope>): <summary>`.
Scopes: `core`, `engine`, `platform`, `keymap`, `app`, `ui`, `harness`, `ci`, `docs`.

```
feat(engine): add release_control command for the controlled side
fix(platform): stop re-injecting remote key events on Windows
refactor(keymap): load per-OS tables from config/keymaps
test(harness): add macos<->linux pair to the scenario matrix
ci: cache cargo builds in the rust job
docs: document the keymap override format
```

Use `!` or a `BREAKING CHANGE:` footer for incompatible protocol or settings changes, and update the contracts in
`docs/contracts/` in the same commit.

## Pull request checklist

- [ ] `just ci` passes locally (or at least `just lint` and `just test`)
- [ ] Tests added or updated; behaviour changes are covered by a harness scenario or unit test
- [ ] Docs/contracts updated in the same PR (no stale README or `docs/contracts/` text)
- [ ] UI changes include light and dark screenshots
- [ ] No new `unwrap()`/`dbg!`/`todo!` in non-test code
- [ ] Conventional commit titles; PR is focused on one concern

## Testing

- **Unit/integration:** `cargo test --workspace`; UI unit tests with `npm test` in `app/ui` (Vitest).
- **Harness:** `just harness` starts N virtual daemons with virtual input devices, runs the OS-pair scenario matrix and
  writes an HTML/JSON report (CI uploads it as the `harness-report` artifact). No hardware needed.
- **E2E:** `just e2e` drives the real UI in headless Chromium with Playwright, against mock or real (virtual-platform)
  daemons. Install the browser once with `cd app/ui && npx playwright install chromium`.

## Screenshots policy

Every milestone that changes UI or behaviour commits a screenshot to `docs/progress/<track>/NN-<milestone>.png`:
UI work as Playwright captures in light and dark; backend work as a rendering of the test or harness output.
Keep each PNG small (viewport-sized, no secrets, no real device names or keys).

## Security

Report vulnerabilities privately; see [`SECURITY.md`](SECURITY.md).
