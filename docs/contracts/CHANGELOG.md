# Contract Changelog

## 0.1.1 — transport decided (non-breaking)

`daemon-ipc.md` previously left the IPC transport "still undecided". Answered, not changed: **WebSocket over loopback TCP, `127.0.0.1:47823`** (`flow_core::ipc::IPC_PORT`), chosen over a Unix domain socket / named pipe so v0.1 needs exactly one implementation across macOS/Windows/Linux. No JSON shape in `data-model.md` or `daemon-ipc.md` changed — every example in both documents was already correct, just previously aspirational. `flow-daemon` (`daemon/src/ipc/{dispatch,server}.rs`) now realizes this contract for real, proven end-to-end by `daemon/tests/ipc_protocol.rs`; `MockDaemonRepository` remains the other implementation this contract governs.

## 0.1.0 — draft (mock-only)

Initial contract, defined by and satisfied entirely by `MockDaemonRepository`. No daemon implements this yet.

- `Device`, `DeviceState` (reusing `flow_core::device::DeviceState`'s six variants), `HostOs`.
- `DaemonLinkState` for daemon/active-link health, independent of per-device state.
- `PairingSession` / `PairingStage` state machine.
- `SwitchKeyBinding`, `FlowSettings`, `PermissionStatus`.
- `DaemonRepository` interface: five `watch*` streams, nine commands.
