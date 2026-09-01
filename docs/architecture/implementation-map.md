# Flow V1 — Implementation Map

Concise map of the code paths that realize the V1 vision (one keyboard + one mouse →
two computers, Scroll Lock switches). Produced from a read of the current `main` /
`product-first-v1` tree. Not a redesign — a description of what exists.

See [`../testing/user-journeys.md`](../testing/user-journeys.md) for the journeys these
paths serve and their status, and [`../../todos-product-first.md`](../../todos-product-first.md)
for the work queue.

## Runtime shape

```
flow-daemon (per machine)
├── IPC listener        127.0.0.1:47823   ← Flutter UI (control plane, token-authed)
├── peer channel        0.0.0.0:<ephemeral>  ← the other daemon (data plane, Noise)
├── discovery           0.0.0.0:47824 UDP  ← announce every 5s + listen
├── hotkey runner       standalone switch-key detection (works with no UI)
└── history logger      connection-history persistence
```

One `DaemonService` (`daemon/src/service/mod.rs`) is the single source of truth: a
`watch`-published device list with **exactly one** `DeviceState::Active`, plus
`DaemonLinkState` (link health) and the settings/trust stores. "Active" = the device input
is sent *to*. `Connected` = paired/reachable. These three are separate, deliberate concepts
— do not collapse them.

## Step-by-step: where each capability lives

| Capability | File(s) | Notes |
|---|---|---|
| Daemon startup / listeners | `daemon/src/main.rs` | `main`, `spawn_peer_channel_listener`, `spawn_discovery`. `fatal()` = clean non-zero exit for unrecoverable misconfig. |
| Dev/test switches | `daemon/src/devmode.rs` | `FLOW_TRACE`, `FLOW_TEST_HOOKS`, `FLOW_SECURITY` (guarded by `FLOW_DEV`). Pure `parse()`, unit-tested. |
| Two-instance config | `main.rs` (`db_path`, `ipc_port`), `ipc/auth.rs` (`token_path`), `discovery/tcp.rs` (`bind_reusable_udp`), `service/mod.rs` (`FLOW_DEVICE_NAME`) | `FLOW_DATA_DIR`, `FLOW_IPC_PORT`, `FLOW_IPC_TOKEN_PATH`, `FLOW_DEVICE_NAME`. Discovery UDP port shared via `SO_REUSEADDR`. |
| Discovery | `daemon/src/discovery/tcp.rs` | `DiscoveryService`: UDP announce/listen, `broadcast_destinations()` enumerates per-interface subnet broadcasts. `instance_id` = own identity key, filters self-announces. |
| Pairing | `daemon/src/channel/handshake.rs`, `service/mod.rs` (`note_discovered_peer`, `pair_with_candidate`, `accept_pairing_request`), `trust/mod.rs` | `PairingRequest`/`PairingDecision` over a `Channel`. Untrusted inbound → `IncomingPairingRequest` prompt to the UI, handshake blocks on consent. |
| Transport (Channels) | `core/src/channel/mod.rs` (trait), `daemon/src/channel/{tcp,noise,negotiate}.rs` | `TcpChannel` (WebSocket) + `NoiseChannel` (ed25519-bound Noise). `connect_best_available` picks TCP. Bluetooth exists behind a feature, **not wired** — ignore for V1. |
| Physical capture | `platform/src/{linux,macos,windows}/capture.rs` behind `core/src/input/mod.rs::InputCapture` | Linux evdev · macOS `CGEventTap` · Windows `WH_KEYBOARD_LL`/`WH_MOUSE_LL`. `DefaultInputCapture` = the adapter this binary was built for. |
| Local suppression | `InputCapture::set_suppress_local` | **Linux:** real (`EVIOCGRAB`). **Windows:** real (`SuppressionGate`, returns `LRESULT(1)`) — merged, unvalidated on HW. **macOS:** real (active tap + raw-FFI trampoline returns `NULL` to drop; `SuppressionGate` port; self-inject guard) — product-first V1 iteration 2, unvalidated on HW. |
| Remote injection | `platform/src/{linux,macos,windows}/injector.rs` behind `InputInjector` | macOS injected mouse-move/left-click fixed in `7673651` (absolute target = current + delta). |
| Routing gate | `daemon/src/pipeline/mod.rs` `is_peer_receiving_input` | Forward a captured event to peer P **iff P is `Active`**. Gates per-peer so a third device gets nothing. Unit-tested, direction confirmed. |
| Full-duplex pipeline | `pipeline/mod.rs` `run_paired_connection` | One `tokio::select!` task: send-while-peer-active + recv-and-inject + suppress toggle on switch + `HeldInputTracker` release on disconnect. |
| Pipeline wiring | `main.rs` `run_peer_pipeline`, `handle_discovered_peer`, `handle_incoming_peer_stream`, `claim_and_run`, `connection_precedence` | Dedup by peer `DeviceId`; simultaneous dial resolved by smaller-identity-key wins. |
| Switch key | `daemon/src/hotkey/{mod,debounce,runner}.rs` | `SwitchKeyMatcher` (pure) + `SwitchDebouncer` (500 ms) + live rebind. Default binding: Scroll Lock. |
| Switch during a live connection | `hotkey/runner.rs` `spawn_pipeline_switch_filter` + `service.enter_peer_pipeline()` / `peer_pipeline_active()` | "Option A": while a peer pipeline runs it owns switch authority (its capture stream still sees the key even while the OS hook withholds it), strips the switch key's KeyDown **and** matching KeyUp from the forwarded stream, and the standalone runner stands down. |
| Switch state transition | `service/mod.rs` `switch_active_device` (IPC) / `switch_active_device_local` (hotkey) | Both maintain the single-`Active` invariant; both emit a `stage="switch"` `hop_note!`. |
| Logging | `daemon/src/logging.rs` | `LevelFilter` reload layer (**not** `EnvFilter` — `RUST_LOG` is ignored today). `flow::hop` target: `hop!` = TRACE firehose, `hop_note!` = DEBUG milestones. `FLOW_TRACE` = global TRACE (pulls in tungstenite noise). **V1 gap.** |
| Persistence | `daemon/src/storage/*` | Single SQLite DB under the data dir: settings, devices (= trust store), identity keypair, history. `DeviceState` deliberately not persisted. |
| UI ↔ daemon | `flutter/lib/data/ipc_daemon_repository.dart`, `flutter/lib/state/*` | Implements the same `docs/contracts/daemon-ipc.md` contract as the Dart mock. `--dart-define=FLOW_DAEMON_MODE=ipc` points it at the real daemon. |

## Known un-resolved concerns (evidence-blocked — need a real two-machine run)

- **#4 connection ownership:** when two paired daemons start together, each ends up with one
  connection it opened + one it accepted; `connection_precedence` is meant to keep exactly
  one. Whether the loser's `close()` ever tears down the winner, and whether the Flutter IPC
  layer misreads that as a fatal daemon disconnect, can only be judged from `FLOW_TRACE`
  logs of a real simultaneous start. Capture `stage=claim|claim_lost|claim_dropped|
  pipeline_up|pipeline_down|link_connected` from both sides.
- **#5 WebSocket 1006:** no abnormal-close handling exists. Need a real capture showing which
  socket (IPC `47823` vs. the peer channel) closed abnormally and the preceding lines.
- **`manual-testing-strategy.md` Tier 0 note:** after a process has opened >1 IPC
  connection, a later connection's detection of a killed daemon can take >40 s. Pre-existing,
  reproduced with raw sockets. Worth its own look if reconnect UX feels wrong.

## macOS suppression — how it works, and what's unverified

`core-graphics` 0.24's safe `CGEventTap` callback returns `Option<CGEvent>`; its trampoline
maps `None` → pass the **original** event through and never returns a null pointer — so it
**cannot drop an event**. `platform/src/macos/capture.rs` therefore calls `CGEventTapCreate`
directly (private `mod ffi`) with its own `extern "C"` trampoline that returns a `NULL`
`CGEventRef` for a withheld event. Supporting pieces:

- active tap (`CGEventTapOptions::Default`), `TapDisabledBy{Timeout,UserInput}` re-arm from
  inside the callback;
- a cross-thread `Arc<AtomicBool>` suppress flag on `MacosInputCapture` (mirrors Windows),
  read every callback;
- `SuppressionGate` (ported from `windows/capture.rs`): withhold a release **iff** its press
  was withheld, so a mid-hold toggle or the switch key's own key-up never strands a
  half-press locally;
- self-inject guard: injected events carry `EVENT_SOURCE_USER_DATA = FLOW_INJECTED_MARKER`
  (`macos/mod.rs`), and the tap passes them through without forwarding or gating.

The callback **fails open**: any panic / borrow conflict / null ctx → the event passes
through untouched (`catch_unwind`), so a bug here never traps the user's own keyboard.

**Unverified on hardware** (no Mac in the dev environment): the `SuppressionGate` is
unit-tested and everything cross-compile-checks + clippy-clean for both apple targets, but
two things can only be confirmed on a real Mac — (1) that returning `NULL` from the raw
callback actually drops the event on the running macOS version, and (2) that
`EVENT_SOURCE_USER_DATA` survives `CGEventPost` (if not, the self-inject guard misses and
Mac-as-master echoes its own input to the peer; fallback is a pid check). Validate on the
maintainer's Mac with a lifeline (SSH session + `killall flow-daemon` ready) — see
`docs/testing/physical-test-script.md` Round 2.
