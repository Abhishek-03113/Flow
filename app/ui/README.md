# Flow UI (React + TypeScript + Vite)

Frontend for the Tauri 2 app in `../src-tauri`. One `FlowClient` talks to an engine through a pluggable `Transport`:

| Transport            | Selected when                          | Notes                                                                                                     |
| -------------------- | -------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `TauriTransport`     | running inside Tauri                   | `invoke('flow_command', {command, payload})`, listens to `flow://event`, sends `resync` after subscribing |
| `WebSocketTransport` | `?daemon=ws://127.0.0.1:47823&token=…` | daemon IPC contract; token is the `Sec-WebSocket-Protocol` subprotocol                                    |
| `MockTransport`      | otherwise                              | in-memory engine; `?scenario=<name>&os=macos\|windows\|linux&fast=1`, `window.__flowMock` test hooks      |

Mock scenarios: `fresh-install two-devices three-devices controlling-remote controlled role-conflict permission-missing connecting reconnecting disconnected error pairing-request no-devices`.

Routes (hash): `#/home #/devices #/shortcuts #/general #/diagnostics #/tray` plus dev-only `#/design` (component gallery) and `#/debug` (engine inspector).

Scripts: `npm run lint | typecheck | test | e2e | build | dev`. Playwright uses the preinstalled Chromium in `$PLAYWRIGHT_BROWSERS_PATH`; `npm run e2e` rewrites the screenshots in `docs/progress/track-c/`.
