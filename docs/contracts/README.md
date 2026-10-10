# UI ↔ Engine Contracts

This directory is the single source of truth for the boundary between the Flow UI (TypeScript, `app/ui`) and the engine (Rust, `daemon/`). The same commands and events are served three ways:

| Transport | Used by | Implemented in |
|---|---|---|
| **Tauri** (`flow_command` invoke + `flow://event`) | the desktop app, engine embedded in-process, no local socket | `app/src-tauri/src/bridge.rs`, `app/ui/src/client/tauriTransport.ts` |
| **WebSocket** (loopback, token) | `flow-daemon` (headless), the app with `FLOW_DEV_WS=1`, browser debugging | `daemon/src/ipc/server.rs`, `app/ui/src/client/webSocketTransport.ts` |
| **Mock** (in-memory) | UI development, Vitest, Playwright | `app/ui/src/client/mockEngine.ts` |

All three must agree on every JSON shape below. The engine side is a single dispatcher (`daemon/src/ipc/dispatch.rs::run_command`) and a single event stream (`daemon/src/ipc/events.rs::StateStream`).

## Documents

- [`data-model.md`](./data-model.md) — the entities that cross the boundary (`Device`, `DaemonLinkState`, pairing types, settings, permissions, control state, layout, diagnostics) and their JSON wire shape.
- [`daemon-ipc.md`](./daemon-ipc.md) — the commands the UI sends, the state the engine pushes, the envelope, error codes and the pairing/switching state machines.
- [`CHANGELOG.md`](./CHANGELOG.md) — contract version history.

## Ground rules

1. **The TypeScript client is the UI-side contract.** [`app/ui/src/client/types.ts`](../../app/ui/src/client/types.ts) and [`flowClient.ts`](../../app/ui/src/client/flowClient.ts) (commands) mirror these docs; the serde types in `core/src` mirror them on the engine side. If a doc and the code disagree they must be reconciled in the same change.
2. **The mock engine must behave like the real one**, including the semantics that matter (what `control_state` means on a controlled device, which fields are `null` before the first measurement). The UI never imports a transport directly, only `FlowClient`.
3. **This is the UI↔engine contract, not the wire protocol between two computers.** `core/src/protocol` defines the keyboard/mouse messages engines exchange; [`docs/architecture/channels.md`](../architecture/channels.md) defines **Channels**, the abstraction that carries them. A UI never sees an `InputEvent` or a `Channel`.
4. **Reuse the Rust vocabulary.** `Device.state` is exactly `flow_core::device::DeviceState`; where a concept is new, `flow-core` defines it and this contract documents it.
5. **Changes bump the contract version** and get a `CHANGELOG.md` entry.
