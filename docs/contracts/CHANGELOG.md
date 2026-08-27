# Contract Changelog

## 0.1.2 — `retry_connection` (non-breaking)

Adds a tenth command, `retry_connection`, closing a gap `daemon-ipc.md`'s own link-health state machine already documented but the interface never implemented: `disconnected --(user retries)--> connecting` and `error --(user retries)--> connecting`. Before this, the Flutter UI's "Retry" affordance (`flutter/lib/features/tray/tray_popover.dart`) had no command to send at all — clicking it just showed a "Reconnected" toast with no daemon round-trip, regardless of whether the link was actually reachable. `retry_connection` only moves the link to `connecting`; it never claims `connected` itself, since real recovery still has to happen (and show up via `link_state_changed`) the normal way. New error code: `link_not_recoverable` (the link isn't `disconnected`/`error`, so there's nothing to retry). Implemented by both `MockDaemonRepository` and `flow-daemon` (`DaemonService::retry_connection`, `daemon/src/ipc/dispatch.rs`).

## No version bump — rust-daemon phase complete

Not a contract revision — recorded here anyway because a reader of this changelog would otherwise have no way to know the daemon phase referenced by 0.1.1 below actually finished. Every task in `daemon/todos.json` (tracks A-J) is now done: discovery, pairing, encryption, trust, replay protection, reconnect, structured logging, and packaging scaffolding for the daemon-to-daemon side of the system — deliberately outside this contract's own scope (`docs/contracts/README.md` ground rule 3: this directory governs the *local* Flutter<->daemon boundary only, never daemon<->daemon). **Nothing in `data-model.md` or `daemon-ipc.md` changed.** The one internal (non-contract) wire type this phase touched, `core::pairing::PairingRequest`, gained a `device_os` field; confirmed before making that change that the type appears nowhere in this directory's own documents, so it carries no contract implication. See `daemon/README.md`'s top status line and `docs/architecture/channels.md` (the daemon<->daemon design doc this contract deliberately doesn't cover) for what actually shipped.

## 0.1.1 — transport decided (non-breaking)

`daemon-ipc.md` previously left the IPC transport "still undecided". Answered, not changed: **WebSocket over loopback TCP, `127.0.0.1:47823`** (`flow_core::ipc::IPC_PORT`), chosen over a Unix domain socket / named pipe so v0.1 needs exactly one implementation across macOS/Windows/Linux. No JSON shape in `data-model.md` or `daemon-ipc.md` changed — every example in both documents was already correct, just previously aspirational. `flow-daemon` (`daemon/src/ipc/{dispatch,server}.rs`) now realizes this contract for real, proven end-to-end by `daemon/tests/ipc_protocol.rs`; `MockDaemonRepository` remains the other implementation this contract governs.

## 0.1.0 — draft (mock-only)

Initial contract, defined by and satisfied entirely by `MockDaemonRepository`. No daemon implements this yet.

- `Device`, `DeviceState` (reusing `flow_core::device::DeviceState`'s six variants), `HostOs`.
- `DaemonLinkState` for daemon/active-link health, independent of per-device state.
- `PairingSession` / `PairingStage` state machine.
- `SwitchKeyBinding`, `FlowSettings`, `PermissionStatus`.
- `DaemonRepository` interface: five `watch*` streams, nine commands.
