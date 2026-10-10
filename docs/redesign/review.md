# Flow 2.0 — final review (Track H)

Fresh-eyes review of the whole redesign (`origin/main...HEAD`, ~557 files) after Tracks A–G landed. Every finding has
an id, a severity and a status; the status column is updated in the commit that fixes it.

Severity: **blocker** (broken for users or unsafe), **major** (wrong behaviour, security hardening or broken tooling a
contributor hits on day one), **minor** (real but contained), **nit** (polish).

Method: read the ADR/plan/contracts, then the engine (router, link, service, IPC, engine wiring), the platform hooks
(`cargo clippy -p flow-platform --target x86_64-pc-windows-gnu|x86_64-apple-darwin -D warnings`: clean), the Tauri
shell, the UI client against the contract, every screen in mock mode at 800 px and 1280 px in light and dark, the CI
workflows, and followed the README/CONTRIBUTING commands literally. Baseline gates were green before any change.

## Findings

| id | severity | area | file:line | finding | status |
|---|---|---|---|---|---|
| H-01 | blocker | UI ↔ contract | `app/ui/src/copy.ts:40`, `app/ui/src/client/types.ts:14` | The engine sets `Device.link_error` to `protocol_mismatch`, `protocol_error`, `hello_timeout`, `connection_lost` or `injector_unavailable` (`daemon/src/link.rs`); the UI only knows `role_conflict`/`version_mismatch`/`unreachable`/`untrusted`. `linkErrorCopy` returns `undefined` for every real code but `role_conflict`, and `DevicesView` reads `err.title` → TypeError, the Computers screen goes blank as soon as a v1 peer or a timed-out hello is seen. | fixed |
| H-02 | major | engine / security | `daemon/src/engine.rs` `handle_incoming_peer_stream`, `daemon/src/service/mod.rs` `accept_incoming_peer_channel`, `accept_pairing_over` | No deadline on an inbound peer connection before it is authenticated: the WebSocket upgrade, the Noise handshake and the wait for the `PairingRequest` can each stall forever. Anything on the LAN can open unbounded sockets/tasks on the peer port (0.0.0.0). Outbound dials already have `DIAL_TIMEOUT`. | fixed |
| H-03 | major | app / security | `app/src-tauri/tauri.conf.json:28` | `"csp": null`: the webview runs with no Content-Security-Policy. The UI needs nothing beyond `'self'`, Tauri IPC and inline styles. | open |
| H-04 | major | app / security | `app/src-tauri/src/lib.rs:25` | The desktop app honours the dev switches in release builds: `FLOW_TEST_HOOKS=1` registers `debug_inject_input`/`debug_virtual_io` on the in-process `flow_command` bridge, and `FLOW_DEV=1 FLOW_SECURITY=insecure` turns off encryption and auto-accepts pairing. Environment variables are easy to leak into a GUI session (launchd/systemd user env); a shipped app should not have these switches at all. | open |
| H-05 | major | contract / CI | `docs/contracts/*`, `daemon/src/ipc/dispatch.rs`, `app/ui/src/client/*` | No automated check that the command/event/error-code/link-error lists agree between the contract, the engine, the TS client, the mock and the UI copy (H-01 and H-11 are exactly that drift). | open |
| H-06 | major | UI layout | `design-system/styles/components.css:1094` | At the 760–820 px window widths the app allows (`minWidth: 760`), Home is 20 px wider than its column and every card is clipped on the right: `.fl-grid` has an implicit `auto` track, so the arrangement canvas' min-content width widens the grid. | open |
| H-07 | major | tooling / docs | `scripts/dev.sh:84,89`, `README.md:82`, `docs/release/bundling-and-release.md:15` | `just dev`, `just build`, the README ("`npx tauri dev` from `app/ui`") and the release doc run the Tauri CLI from `app/ui`, where it panics: "Couldn't recognize the current folder as a Tauri project". It must run from `app/`. | open |
| H-08 | major | platform / Linux | `platform/src/linux/capture.rs:114` | Linux suppression is a whole-device `EVIOCGRAB`, so a `Disposition::Pass` while grabbed is not delivered anywhere: with *Share keyboard* (or *Share mouse*) off, a Linux controller's own keyboard is dead while a remote screen is active; and keys held when the switch happens (e.g. the modifiers of a `Ctrl+Alt+→` switch chord) never get their release locally, so they stay down in the local session until pressed again. Fix needs a uinput pass-through for `Pass` events while grabbed, which must be validated on hardware. | open |
| H-09 | minor | tests | `app/ui/e2e/helpers.ts:4`, `app/ui/e2e-live/fixtures.ts:13` | `npm run e2e` and `npm run e2e:live` rewrite tracked PNGs under `docs/progress/track-{c,e,g}/` on every run, leaving the tree dirty (reported by the orchestrator). | fixed |
| H-10 | minor | CI | `.github/workflows/ci.yml` (ui, ui-e2e-live), `release.yml` | The ui job uploads `app/ui/screenshots/` (does not exist); the live job uploads `docs/progress/track-g/` (see H-09); `release.yml` points `rust-cache` at `app/src-tauri`, but the workspace target dir is the repo root, so the release cache never hits. | open |
| H-11 | minor | UI copy | `app/ui/src/copy.ts:5` | No friendly copy for the engine codes `no_controller`, `permission_not_found`, `not_reconnecting` (they fall back to "Something went wrong"). | open |
| H-12 | minor | docs | `docs/architecture/implementation-map.md` | Describes the removed V1 data path (`pipeline/mod.rs`, `ownership.rs`, `hotkey/runner.rs`, `OwnershipChanged`, Primary/Secondary). There is no current architecture overview or diagram. | open |
| H-13 | minor | docs | `README.md:90-104`, `daemon/README.md:159,173,300`, `daemon/src/ipc/server.rs:322`, `daemon/examples/drive_two_daemons.rs:194`, `platform/src/windows/{mod,inject_translate}.rs` | Stale text: the README roadmap says macOS/Windows have no local suppression and refers to `main.rs`/pipeline tracks; daemon docs and comments refer to the removed hotkey runner and pipeline and to the contract as "Flutter<->daemon"; platform comments use "slave". | open |
| H-14 | minor | IPC security | `daemon/src/ipc/auth.rs:113`, `daemon/src/ipc/server.rs:75` | The token file is written with the default umask and `chmod 600`ed afterwards (brief world-readable window on first run); the presented token is compared with `==` (not constant time). Low impact (loopback, 256-bit token) but cheap to fix. | open |
| H-15 | minor | app | `app/src-tauri/src/integrations.rs:61` | With the tray icon turned off, *every* `settings_changed` (any setting, any resync) calls `show_main`, popping a minimized window back up. Only the transition to hidden needs it. | open |
| H-16 | nit | app copy | `app/src-tauri/src/integrations.rs:105` | The pairing notification says "*X* wants to share its keyboard and mouse" whatever the requester's role; a controlled computer asking to pair does not. | open |
| H-17 | nit | contract docs | `docs/contracts/daemon-ipc.md` | `open_main_window` (answered by the desktop shell, never the engine) is used by the tray popover but not documented. | open |
| H-18 | minor | platform / Windows | `platform/src/windows/capture.rs` | Windows silently removes a low-level hook whose callback exceeds `LowLevelHooksTimeout`; nothing detects or reinstalls it (the macOS tap re-arms itself). The callback is short and lock-light, so this is a hardening item to validate on hardware. | open |
| H-19 | nit | tooling | `scripts/dev.sh:62` | `just lint` runs prettier twice (`npm run lint` already includes `prettier --check`). | open |
| H-20 | minor | a11y | `app/ui` | No automated accessibility check over the main routes. | open |
| H-21 | nit | UI | `design-system/styles/components.css` (`.fl-tip__bubble`) | A tooltip on a right-edge control (Forget on Computers) overflows the viewport by ~10 px. | open |
| H-22 | nit | engine | `daemon/src/storage/*.rs` | SQLite errors are `expect`ed (a full disk panics the task that hit it). Pre-existing, isolated per task, and not reachable from network data. | won't fix now: needs a storage error type threaded through every repo; out of scope for polish |

## Verified, no finding

- **Input routing** (`daemon/src/router.rs`): one capture/one lock; suppression on before `Enter`, off after `Leave`;
  up-follows-down via the held table (releases of keys sent to a left screen are swallowed, since the controlled side
  releases everything on `Leave`); the completing hotkey key is swallowed down and up; link loss while remote returns
  home; edge dwell and warp-settle logic is sound. The controlled side ignores `held_modifiers` (so nothing can stick
  there) and injects only between `Enter` and `Leave`, with a replay guard, and releases all on every exit path
  (`link.rs` `controlled_loop`).
- **Link loops**: `select!` arms are cancel-safe (`WebSocketStream::next`, Noise decrypt is synchronous after the frame
  arrives); heartbeats and a liveness timeout on both sides; `Hello` has a timeout; refusals are surfaced and stop
  redialling only when redialling cannot help.
- **No `unwrap`/`expect` on network data** in non-test engine code (all remaining ones are invariants or local storage,
  H-22). Clippy's `await_holding_lock` is on and clean.
- **Platform hooks**: Windows ignores `LLKHF_INJECTED`/`LLMHF_INJECTED` and Flow's marker, swallows synchronously,
  catches panics before the FFI boundary and fails open; the `SetCursorPos` recentre echo is filtered. macOS uses an
  active HID tap, re-arms on `TapDisabledBy*`, filters its own injected events by `EVENT_SOURCE_USER_DATA`, fails open
  on panic, and parks the cursor while suppressing. Both cross-target clippy runs are clean.
- **Pairing**: identity is the Noise-proven public key, never a name; the verification code (16 hex digits, SHA-256 over both public keys in canonical order) is derived from both keys
  and shown on both sides; inbound requests need an explicit Accept (none without a UI, 30 s timeout, one at a time,
  withdrawn when the initiator cancels). Insecure mode needs both `FLOW_DEV=1` and `FLOW_SECURITY=insecure` and prints a
  banner (headless daemon; see H-04 for the app).
- **IPC**: bound to `127.0.0.1` only; token via `Sec-WebSocket-Protocol` (a page cannot read the token file); 256 KiB
  frame cap; test-hook commands answer `unknown_command` unless `FLOW_TEST_HOOKS` is set.
- **Shell-side opens**: `reveal_config_dir`/`open_permission_settings` only ever open the engine's own config folder or
  a fixed per-OS settings URL; the webview has no opener permission; IPC payloads never name a path (`get_keymap` takes
  an OS enum, remap rules go to a fixed `remap.toml`), so there is no traversal surface.
- **Diagnostics export**: settings, devices (no `public_key`), link/permission state, counters and the log tail; logs
  carry no tokens, keys or pairing codes.
- **Architecture**: one dispatcher (`ipc::dispatch::run_command`, used by both the WebSocket server and the Tauri
  bridge), one event stream (`ipc::events::StateStream`), one config builder (`env_config::engine_config`, used by the
  daemon binary and the app); the baton model's `OwnershipChanged`/`not_primary`/pipeline code is gone.
  `switch_active_device` is kept as a documented alias; `SWITCH_DEBOUNCE_WINDOW` is the hotkey debounce, not baton code.
