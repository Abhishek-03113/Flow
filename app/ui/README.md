# Flow UI (React + TypeScript + Vite)

Frontend for the Tauri 2 app in `../src-tauri`. One `FlowClient` talks to an engine through a pluggable `Transport`:

| Transport            | Selected when                          | Notes                                                                                                     |
| -------------------- | -------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `TauriTransport`     | running inside Tauri                   | `invoke('flow_command', {command, payload})`, listens to `flow://event`, sends `resync` after subscribing |
| `WebSocketTransport` | `?daemon=ws://127.0.0.1:47823&token=…` | daemon IPC contract; token is the `Sec-WebSocket-Protocol` subprotocol                                    |
| `MockTransport`      | otherwise                              | in-memory engine; `?scenario=<name>&os=macos\|windows\|linux&fast=1`, `window.__flowMock` test hooks      |

Mock scenarios: `fresh-install two-devices three-devices controlling-remote controlled role-conflict permission-missing connecting reconnecting disconnected error pairing-request no-devices`.

Routes (hash): `#/home #/devices #/shortcuts #/general #/diagnostics #/tray` plus dev-only `#/design` (component gallery) and `#/debug` (engine inspector).

Scripts: `npm run lint | typecheck | test | e2e | e2e:live | build | dev`. Playwright uses the preinstalled Chromium in `$PLAYWRIGHT_BROWSERS_PATH`; `npm run e2e` rewrites the screenshots in `docs/progress/track-c/`.

## Live E2E (`npm run e2e:live`)

`e2e/` drives the UI against `MockTransport`; `e2e-live/` (config `playwright.live.config.ts`) drives the same UI against **real engines**: each test starts its own `flow-harness up` world (virtual devices with real control servers, ephemeral ports), opens one browser context per device on `?daemon=<ws_url>&token=<token>`, and checks both what the pages show and what the engines did (`debug_virtual_io`).

| File                       | What it holds                                                                                                                                                                                                                                                                                                              |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `e2e-live/harness.ts`      | `Harness` (spawn `flow-harness up --stop-on-eof` in its own process group, wait for the manifest, `disconnect`/`reconnect` a device over stdin, SIGINT then SIGKILL the group on stop) and `DaemonClient` (typed control-API client: `call`, `inject`/`tap`/`chord`/`move`, `virtualIo`/`clearIo`, latest event payloads). |
| `e2e-live/fixtures.ts`     | The `world` option (`test.use({ world: { devices, paired, security } })`) and the `live` fixture: `live.daemon(name)`, `live.open(name, { onboarding, hash, theme })`, `live.disconnect/reconnect(name)`; `composite()` writes side-by-side screenshots to `docs/progress/track-g/`.                                       |
| `e2e-live/global-setup.ts` | `cargo build -p flow-harness --release` (a no-op when up to date); `FLOW_HARNESS_BIN=<path>` uses a prebuilt binary instead.                                                                                                                                                                                               |
| `e2e-live/*.spec.ts`       | The journeys: pairing (onboarding, codes, cancel), switching (UI, hotkey, release), link loss, role conflict, settings, arrangement, remaps and key map, three computers, diagnostics.                                                                                                                                     |

Run with `CARGO_TARGET_DIR` pointing at your usual target directory if you set one. Waits are `expect.poll`/locator waits only; the suite serves the built UI on port 4174 (the mock suite uses 4173).
