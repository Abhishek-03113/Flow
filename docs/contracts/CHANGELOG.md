# Contract Changelog

## No version bump — rust-daemon phase complete

Not a contract revision — recorded here anyway because a reader of this changelog would otherwise have no way to know the daemon phase referenced by 0.1.1 below actually finished. Every task in `daemon/todos.json` (tracks A-J) is now done: discovery, pairing, encryption, trust, replay protection, reconnect, structured logging, and packaging scaffolding for the daemon-to-daemon side of the system — deliberately outside this contract's own scope (`docs/contracts/README.md` ground rule 3: this directory governs the *local* Flutter<->daemon boundary only, never daemon<->daemon). **Nothing in `data-model.md` or `daemon-ipc.md` changed.** The one internal (non-contract) wire type this phase touched, `core::pairing::PairingRequest`, gained a `device_os` field; confirmed before making that change that the type appears nowhere in this directory's own documents, so it carries no contract implication. See `daemon/README.md`'s top status line and `docs/architecture/channels.md` (the daemon<->daemon design doc this contract deliberately doesn't cover) for what actually shipped.

## 0.1.1 — medium decided (non-breaking)

`daemon-ipc.md` previously left the Control Link medium "still undecided". Answered, not changed: **WebSocket over loopback TCP, `127.0.0.1:47823`** (`flow_core::ipc::IPC_PORT`), chosen over a Unix domain socket / named pipe so v0.1 needs exactly one implementation across macOS/Windows/Linux. No JSON shape in `data-model.md` or `daemon-ipc.md` changed — every example in both documents was already correct, just previously aspirational. `flow-daemon` (`daemon/src/ipc/{dispatch,server}.rs`) now realizes this contract for real, proven end-to-end by `daemon/tests/ipc_protocol.rs`; `MockDaemonRepository` remains the other implementation this contract governs.

## 0.1.0 — draft (mock-only)

Initial contract, defined by and satisfied entirely by `MockDaemonRepository`. No daemon implements this yet.

- `Device`, `DeviceState` (reusing `flow_core::device::DeviceState`'s six variants), `HostOs`.
- `DaemonLinkState` for daemon/active-link health, independent of per-device state.
- `PairingSession` / `PairingStage` state machine.
- `SwitchKeyBinding`, `FlowSettings`, `PermissionStatus`.
- `DaemonRepository` interface: five `watch*` streams, nine commands.
