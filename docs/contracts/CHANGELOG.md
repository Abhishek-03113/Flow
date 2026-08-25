# Contract Changelog

## 0.1.0 — draft (mock-only)

Initial contract, defined by and satisfied entirely by `MockDaemonRepository`. No daemon implements this yet.

- `Device`, `DeviceState` (reusing `flow_core::device::DeviceState`'s six variants), `HostOs`.
- `DaemonLinkState` for daemon/active-link health, independent of per-device state.
- `PairingSession` / `PairingStage` state machine.
- `SwitchKeyBinding`, `FlowSettings`, `PermissionStatus`.
- `DaemonRepository` interface: five `watch*` streams, nine commands.
