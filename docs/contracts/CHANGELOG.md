# Contract Changelog

## 0.2.2 — behaviour found by the live UI suite (no shape changes)

- **Refused links are `error`, not `reconnecting`**: a link refused for `role_conflict` or `protocol_mismatch` sets the link state to `error` (the device's `link_error` says why) and the engine stops dialling that peer until this device's role changes; the peer, when its role changes, dials back and the link comes up with no `retry_connection`. A controlled device refusing a second controller is the exception: that controller keeps dialling, since the controlled device may accept it once its current controller leaves.
- **`Device.role` / `os` follow the refused `Hello`**: a peer that changed its role shows its new role even though no link came up.
- **Key tokens**: switch keys and remap rules accept the UI's key tokens (`Space`, `Esc`, `Enter`, `Backspace`, `Delete` = forward delete, `CapsLock`, `PageUp`, `Up`, `` ` ``, `-`, ...) as well as wire key names (`ESCAPE`, `UP_ARROW`, ...); both name the same key. Before, a named key recorded in the UI never matched (`Ctrl + Up` did not switch) and remap rules with one were refused as `invalid_remap_rule`.
- **`get_keymap`** answers the keymap file `{ version, os, keys: [{ name, code }] }` (as documented); the UI had assumed `{ os, entries }`.
- **Incoming pairing requests are withdrawn** when the initiator cancels (its connection closes): `incoming_pairing_request_changed` goes back to `null`.

## 0.2.1 — embedded engine and honest permissions (additive, one rename)

The desktop app now runs the engine in-process; the same command/event contract is served by the Tauri bridge (`flow_command` + `flow://event`) and, for the headless daemon and debugging (`FLOW_DEV_WS=1`), by the WebSocket.

- **Ack envelope**: an ack that carries an answer uses **`result`** (`{ id, ok: true, result }`), the engine's name since 0.2.0. Clients that read `data` must read `result`; the Tauri bridge resolves `flow_command` with the same value (`null` for a plain ack).
- **New command `resync {}`**: the engine re-sends all ten state events (the Tauri equivalent of connecting).
- **`Device.is_local`** (bool): explicit; clients no longer guess from the id.
- **`PairingSession.code`**: the verification code while `requesting`, identical to the other computer's `IncomingPairingRequest.fingerprint` (derived from both public keys).
- **`PermissionStatus.items`**: `[{ name, granted, how_to_fix: { summary, steps: [{ text, command? }] } | null }]`. macOS reports Accessibility and Input Monitoring separately, Linux reading input devices and creating the virtual one, Windows none. `granted` is true when all are. The engine re-probes every 2 s; `request_permission` shows the real OS prompt where one exists.
- **New commands**: `recheck_permissions` ("Check Again"), `open_permission_settings {name}` (answers `{ url }`; the app opens it), `reveal_config_dir` (answers `{ path }`; the app opens the folder), `export_diagnostics` (writes a JSON bundle, answers `{ path }`), `cancel_reconnect` (stops redialling after a lost link until `retry_connection`; only while `reconnecting`).
- **New error codes**: `permission_not_found`, `not_reconnecting`.
- **Behaviour**: `last_seen` / `last_seen_ms` now mean "last heard from" (heartbeat), not "last switched to". `launch_at_login` defaults to `false` and is honoured by the app, as are `show_tray_icon` and `auto_connect_paired_devices` (off: Flow does not dial paired computers, it only accepts their connections).
- **`ControlState`** clarified: on a controlled device `active_screen` is always `"local"`; `entered` says whether the controller is driving it. The UI was reading it the other way round and was fixed.
- **Link state with no paired computer**: the engine reports `disconnected`; UIs show the neutral "No computers yet", not an error.
- `Diagnostics.switch_latency_ms_*` is `null` until the first switch; UIs show "—".

## 0.2.0 — controller/controlled model (breaking for switching, additive elsewhere)

Implements [ADR 0001](../adr/0001-controller-controlled-input-model.md). The symmetric "ownership baton" is gone; control has a static **role** per device and the active screen lives only on the controller.

- **Settings**: `role` (`controller` | `controlled`, default `controller`), `return_home_key` (`SwitchKeyBinding | null`; a patch sets it, an explicit `null` clears it), `edge_switching` (default `true`), `edge_switch_delay_ms` (default `0`), `keymap_overrides_path` (read-only, set by the engine, never patched or persisted). `share_keyboard`, `share_mouse` and `pointer_sensitivity` (low 0.6 / normal 1.0 / high 1.6) are now honoured by the router.
- **`Device`** gains `role`, `screen {width,height}` (both `null` until the peer said hello), `link_error` (e.g. `role_conflict`, `protocol_mismatch`) and `last_seen_ms` (epoch ms, derived from `last_seen`, which stays). Exactly the active screen's device is `active`; a peer with a live link is `inactive`, one without `disconnected`, one whose link was refused `error`.
- **New types**: `ControlState {role, active_screen: "local" | DeviceId, controller_device_id, entered}` (`entered` is true on a controlled device while the controller drives it), `ScreenLayout {screens: [{device_id, x, y, width, height}]}` (this computer is `d1`), `Diagnostics {switch_latency_ms_p50, switch_latency_ms_p95, events_forwarded, events_dropped, uptime_s}`, `RemapRule {from, to, when_target_os}`.
- **New commands**: `switch_to {target: "local" | DeviceId}` (controller; `switch_active_device` stays as an alias and treats `d1` as `local`), `release_control` (controlled), `set_role {role}`, `set_screen_layout {layout}` (controller), `set_remap_rules {rules}`, `get_keymap {os}`. `get_keymap` is the first command with an answer: its response is `{ "id", "ok": true, "result": <keymap file> }` (`IpcResponse::Data`).
- **New events**, sent on connect after the original six: `control_state_changed`, `remap_rules_changed`, `screen_layout_changed`, `diagnostics_changed` (then every ~2s).
- **New error codes**: `not_controller`, `not_controlled`, `no_controller`, `invalid_remap_rule`, `invalid_layout`, `internal_error`.
- **Changed**: `switch_to` / `switch_active_device` no longer wait 400ms and require a *live link* to the target (`device_not_switchable` otherwise), not merely an `inactive`/`connected` state.
- **Daemon-to-daemon protocol** (not part of this contract, listed for reference) is now `PROTOCOL_VERSION = 2`; v1 peers are refused.
- Not done: `reveal_config_dir` and `export_diagnostics` from the plan are not implemented by the engine.

## 0.1.3 — incoming pairing prompt (non-breaking)

Adds a sixth `watch*` stream, `watchIncomingPairingRequest()` (event
`incoming_pairing_request_changed`, sent last on connect, `null` when
nothing is pending), and an eleventh command,
`respond_to_pairing_request` (`{ request_id, decision }`). Together they
replace the daemon's internal, undocumented stand-in for user consent —
a stage-based "pairing window" that only accepted an incoming request
while this device's own user happened to be mid-pair — with a real
Accept/Reject prompt: any untrusted inbound connection raises an
`IncomingPairingRequest` on the UI, and the user's choice drives the
handshake. No connected UI ⇒ immediate reject; ~30s without an answer ⇒
reject; one request at a time. New error code: `pairing_request_not_found`.
New type `IncomingPairingRequest` in `data-model.md`. Implemented by both
`MockDaemonRepository` and `flow-daemon`
(`DaemonService::respond_to_pairing_request`,
`daemon/src/service/mod.rs`; `daemon/src/ipc/dispatch.rs`).

## 0.1.2 — `retry_connection` (non-breaking)

Adds a tenth command, `retry_connection`, closing a gap `daemon-ipc.md`'s own link-health state machine already documented but the interface never implemented: `disconnected --(user retries)--> connecting` and `error --(user retries)--> connecting`. Before this, the Flutter UI's "Retry" affordance (`flutter/lib/features/tray/tray_popover.dart`) had no command to send at all — clicking it just showed a "Reconnected" toast with no daemon round-trip, regardless of whether the link was actually reachable. `retry_connection` only moves the link to `connecting`; it never claims `connected` itself, since real recovery still has to happen (and show up via `link_state_changed`) the normal way. New error code: `link_not_recoverable` (the link isn't `disconnected`/`error`, so there's nothing to retry). Implemented by both `MockDaemonRepository` and `flow-daemon` (`DaemonService::retry_connection`, `daemon/src/ipc/dispatch.rs`).

## No version bump — rust-daemon phase complete

Not a contract revision — recorded here anyway because a reader of this changelog would otherwise have no way to know the daemon phase referenced by 0.1.1 below actually finished. Every task in `docs/archive/tasks/daemon-todos.json` (tracks A-J) is now done: discovery, pairing, encryption, trust, replay protection, reconnect, structured logging, and packaging scaffolding for the daemon-to-daemon side of the system — deliberately outside this contract's own scope (`docs/contracts/README.md` ground rule 3: this directory governs the *local* Flutter<->daemon boundary only, never daemon<->daemon). **Nothing in `data-model.md` or `daemon-ipc.md` changed.** The one internal (non-contract) wire type this phase touched, `core::pairing::PairingRequest`, gained a `device_os` field; confirmed before making that change that the type appears nowhere in this directory's own documents, so it carries no contract implication. See `daemon/README.md`'s top status line and `docs/architecture/channels.md` (the daemon<->daemon design doc this contract deliberately doesn't cover) for what actually shipped.

## 0.1.1 — transport decided (non-breaking)

`daemon-ipc.md` previously left the IPC transport "still undecided". Answered, not changed: **WebSocket over loopback TCP, `127.0.0.1:47823`** (`flow_core::ipc::IPC_PORT`), chosen over a Unix domain socket / named pipe so v0.1 needs exactly one implementation across macOS/Windows/Linux. No JSON shape in `data-model.md` or `daemon-ipc.md` changed — every example in both documents was already correct, just previously aspirational. `flow-daemon` (`daemon/src/ipc/{dispatch,server}.rs`) now realizes this contract for real, proven end-to-end by `daemon/tests/ipc_protocol.rs`; `MockDaemonRepository` remains the other implementation this contract governs.

## 0.1.0 — draft (mock-only)

Initial contract, defined by and satisfied entirely by `MockDaemonRepository`. No daemon implements this yet.

- `Device`, `DeviceState` (reusing `flow_core::device::DeviceState`'s six variants), `HostOs`.
- `DaemonLinkState` for daemon/active-link health, independent of per-device state.
- `PairingSession` / `PairingStage` state machine.
- `SwitchKeyBinding`, `FlowSettings`, `PermissionStatus`.
- `DaemonRepository` interface: five `watch*` streams, nine commands.
