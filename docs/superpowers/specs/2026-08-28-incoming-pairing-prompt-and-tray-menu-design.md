# Incoming pairing prompt (end-to-end) + tray-icon native menu — design

Date: 2026-08-28
Branch: `feat/pairing-prompt-and-tray-menu`
Status: design, pending review

Related: GitHub issue #18 ("connection over lan gets refused due to
'declined an incoming pairing request'").

---

## Problem

### Track 1 — cross-device pairing is unreliable

`DaemonService::accept_pairing_over` (`daemon/src/service/mod.rs:863`)
refuses any incoming pairing request unless this daemon's **own**
`pairing_session.stage != Idle` (`is_pairing_window_open`, `mod.rs:850`).
That window auto-closes ~1600ms after this side's own attempt reaches
`Paired`/`Failed` (`schedule_terminal_to_idle`,
`PAIRING_TERMINAL_TO_IDLE`).

Consequences, both observed:

- Two users who each press "Pair a device" and select each other still
  fail whenever their clicks are more than ~1.6s apart: the first side to
  finish returns to `Idle`, and the second side's in-flight request — or
  discovery's 5s re-announce redial retry — lands on an `Idle` daemon and
  is declined (`declined an incoming pairing request: no pairing session
  is open on this device`, issue #18).
- If one side's outbound dial fails (asymmetric LAN reachability, e.g. a
  firewalled peer listener), that side gives up and closes its window, so
  the peer's *working* inbound direction is refused too.

There is no incoming-request prompt in the contract
(`docs/contracts/daemon-ipc.md`) or the UI. The window is an explicit,
documented stand-in for one (`mod.rs:811-819`, `830-849`;
`daemon/tests/pairing_over_channel.rs:67-75`).

**Fix:** replace the stage-based window with a real prompt. The daemon
asks the connected UI; the user's Accept/Reject drives the handshake. The
UI prompt is the *only* gate.

### Track 2 — the tray icon opens the app window instead of a menu

`_RealApp.onTrayIconMouseDown` (`flutter/lib/app.dart:200`) toggles the
whole `AppWindowShell`. The designed tray surface
(`product/design` branch, `design/claude-design-export/TrayPopover.dc.html`)
is a compact control panel — status, active device, click-to-switch
device rows, inline "Pair New Device", and Dashboard/Settings/Quit rows —
and only Dashboard/Settings should open the window. `TrayPopover` is
fully built and tested but mounted only by the dev harness.

**Fix (interim):** left-click opens a native `tray_manager` menu built
from live daemon state. The visually-rich popover panel (needs a second
window or a single-window mode-switch) is tracked as follow-up, not in
this spec.

---

## Decisions taken (from brainstorming)

| Question | Decision |
|---|---|
| Incoming request, no UI connected | Reject immediately (can't consent). |
| Prompt surface | Modal dialog; bring the app window to front. |
| Identity shown | Device name + OS + short fingerprint of the peer's proven key. |
| Receiver must be pairing first? | No — cold prompt. The prompt is the whole gate. |
| Daemon waits for decision via | `oneshot` channel + 30s timeout. |
| Tray rendering | Rich native menu from live state (interim). |

Out of scope (future spec): numeric-match SAS; "block this device" /
rate-limiting beyond one-at-a-time; showing the fingerprint on the
initiator side; the glass tray popover panel + its inline pairing flow.

---

## Track 1 — incoming pairing prompt

### 1.1 Contract (`docs/contracts/`)

Version → **0.1.3** (additive; same precedent as `retry_connection` at
0.1.2).

**New state stream** — modeled as state, not a fire-and-forget
notification, so a UI that reconnects mid-prompt still sees the pending
request:

```dart
Stream<IncomingPairingRequest?> watchIncomingPairingRequest();
```

- Event name: `incoming_pairing_request_changed`.
- Pushed on connect like the other five, initially `null`. The "exactly
  five initial events in order" rule (`daemon-ipc.md`, and the
  `ipc/server.rs` / `ipc_protocol.rs` tests) becomes **six**, with
  `incoming_pairing_request_changed` last.

**New command:**

```dart
Future<void> respondToPairingRequest(String requestId, PairingDecision decision);
```

| Command | Payload | Preconditions | Effect |
|---|---|---|---|
| `respond_to_pairing_request` | `{ request_id, decision: "accept" \| "reject" }` | A request with `request_id` is currently pending. | Unblocks the daemon's handshake with that decision; clears the pending request (`incoming_pairing_request_changed: null`). On `accept`, the peer is persisted and appears via `devices_changed`. |

New error code: **`pairing_request_not_found`** — no pending request with
that id (already timed out, withdrawn, or a stale id). The Flutter
dialog treats this as "already resolved" and just dismisses.

**`data-model.md`** — new type:

```dart
class IncomingPairingRequest {
  final String requestId;   // opaque, daemon-generated (uuid v4)
  final String deviceName;  // self-reported by the peer; display only
  final HostOs deviceOs;    // self-reported by the peer
  final String fingerprint; // e.g. "3f2a 91c4 8d10 6b57" — short hash of
                            // the peer's PROVEN ed25519 public key
  final String address;     // peer source IP, display only
}
```

**`CHANGELOG.md`** — new `## 0.1.3 — incoming pairing prompt
(non-breaking)` entry: describes the new stream + command + error code,
notes it removes the daemon's internal stage-based
`is_pairing_window_open` gate (not a contract type), and that both
`MockDaemonRepository` and `flow-daemon` implement it.

### 1.2 `flow_core`

- `PairingDecision` (`core/src/pairing/mod.rs`) already exists — reuse.
- **New** `IncomingPairingRequest` struct, serde `rename_all =
  "snake_case"`, mirrors the data-model type field-for-field.
- **New** `pub fn key_fingerprint(public_key: &[u8]) -> String` in
  `core/src/pairing/mod.rs`: SHA-256 of the key, first 8 bytes rendered
  as four space-separated lowercase hex groups
  (`"{:04x} {:04x} {:04x} {:04x}"`). Unit test pins the output for a
  fixed key so daemon and any future initiator-side display agree.
  - `sha2` is already a workspace dependency via the daemon; add it to
    `core/Cargo.toml` if not already present there.

### 1.3 Daemon

**`daemon/src/channel/handshake.rs`** — the current
`respond_to_pairing(channel, decide: impl FnOnce(&PairingRequest) ->
PairingDecision)` forces a synchronous decision. Split it:

```rust
pub async fn recv_pairing_request(channel: &mut dyn Channel)
    -> Result<PairingRequest, ChannelError>;

pub async fn send_pairing_decision(channel: &mut dyn Channel, decision: PairingDecision)
    -> Result<(), ChannelError>;
```

Keep `respond_to_pairing` as a thin wrapper (`recv` then `send`) for the
sync test callers already in this file.

**`daemon/src/service/mod.rs`:**

- `ServiceState` gains:
  ```rust
  incoming_request: Option<PendingPairingRequest>,
  ```
  where
  ```rust
  struct PendingPairingRequest {
      info: IncomingPairingRequest,
      responder: oneshot::Sender<PairingDecision>,
  }
  ```
- `DaemonService` gains `incoming_request_tx:
  watch::Sender<Option<IncomingPairingRequest>>` and
  `pub fn watch_incoming_request(&self) ->
  watch::Receiver<Option<IncomingPairingRequest>>`.
- **Delete** `is_pairing_window_open` and its two unit tests
  (`an_incoming_pairing_request_is_rejected_when_no_pairing_window_is_open`,
  `..._accepted_while_a_pairing_window_is_open`). The `PAIRING_*` timing
  consts stay — they still drive the initiator-side UI stage machine.
- **New** const `PAIRING_DECISION_TIMEOUT: Duration =
  Duration::from_secs(30)`.
- Rewrite `accept_pairing_over(channel, peer_public_key)`:
  1. `let request = handshake::recv_pairing_request(channel).await?;`
  2. **No UI connected** (`self.connected_clients.load(Relaxed) == 0`) →
     `send_pairing_decision(channel, Reject).await?;` return `Ok(())`.
     `tracing::info!` a one-line "declined: no UI connected to prompt".
  3. **One already pending** (`state.incoming_request.is_some()`) →
     `send_pairing_decision(channel, Reject).await?;` return `Ok(())`.
     `tracing::info!` "declined: another pairing request is awaiting a
     decision".
  4. Build `IncomingPairingRequest { request_id: Uuid::new_v4(),
     device_name: request.device_name.clone(), device_os:
     request.device_os, fingerprint:
     key_fingerprint(&peer_public_key), address: <peer ip or ""> }`.
     Create `oneshot::channel()`. Store `PendingPairingRequest` in
     `state.incoming_request`; `incoming_request_tx.send_replace(Some(
     info.clone()))`.
  5. `let decision = tokio::select! {
        d = rx => d.unwrap_or(PairingDecision::Reject),
        _ = tokio::time::sleep(PAIRING_DECISION_TIMEOUT) => PairingDecision::Reject,
     };`
  6. Clear: `state.incoming_request = None;`
     `incoming_request_tx.send_replace(None);`
  7. `send_pairing_decision(channel, decision).await?;`
  8. On `Accept` — persist the peer exactly as today (upsert
     `DeviceRecord` with the proven `public_key`, `emit_devices()`).
- **New** `pub async fn respond_to_pairing_request(&self, request_id:
  &str, decision: PairingDecision) -> Result<(), FlowError>`:
  under the state lock, if `state.incoming_request` matches
  `request_id`, `take()` it and `.responder.send(decision)` (ignore a
  send error — means step 5 already timed out; the next `send_replace(
  None)` in `accept_pairing_over` still fires). Otherwise
  `Err(FlowError::PairingRequestNotFound)`. Do **not** clear state or
  emit here — `accept_pairing_over` step 6 is the single owner of that,
  so there is exactly one clear+emit per request.
- `from_storage` / `new_seeded_for_test`: `incoming_request: None`.
- Address for step 4: `accept_incoming_peer_channel` /
  `accept_pairing_request` currently take `Box<dyn Channel>` with no
  peer address exposed. Add an optional `peer_addr: Option<SocketAddr>`
  parameter threaded from `main.rs`'s accept loop (it has the
  `TcpStream` and its `peer_addr()`); render as a bare IP string, `""`
  when unknown (e.g. Bluetooth). Display only — never trusted.

**`daemon/src/service` connected-client count:** `DaemonService` gains
`connected_clients: Arc<AtomicUsize>`. `ipc/server.rs::handle_connection`
does `clients.fetch_add(1, Relaxed)` after a successful handshake and
`fetch_sub(1, Relaxed)` on any exit path (guard struct with a `Drop`
impl). `main.rs` constructs the `Arc` and hands clones to the service
and the server.

**`core/src/error.rs`** — `FlowError::PairingRequestNotFound`; `From<
FlowError> for ErrorPayload` maps it to code `pairing_request_not_found`.

**`daemon/src/ipc/dispatch.rs`** — new arm:
```rust
"respond_to_pairing_request" => {
    let args: RespondToPairingRequestPayload = parse_payload(payload)?;
    service.respond_to_pairing_request(&args.request_id, args.decision)
        .await.map_err(ErrorPayload::from)
}
```
with `struct RespondToPairingRequestPayload { request_id: String,
decision: PairingDecision }` (serde; `PairingDecision` already
`snake_case`).

**`daemon/src/ipc/server.rs`** — subscribe `incoming_request_rx`; send
`incoming_pairing_request_changed` as the 6th initial event; add a
`select!` arm forwarding its changes. Bump the "exactly five initial
events" test to six and add the new name to the expected order.

**`daemon/src/main.rs`** — thread `peer_addr` into the pairing accept
path; construct and share the `connected_clients` `Arc`.

**`daemon/README.md`** — rewrite the paragraph at line ~289 ("accepts an
incoming pairing request **only while this daemon's own user has a
pairing session open**") to describe the prompt: any untrusted inbound
request raises a prompt on the connected UI; no UI ⇒ immediate reject;
30s no answer ⇒ reject; one at a time.

### 1.4 Flutter

- **`domain/pairing.dart`** — `IncomingPairingRequest` model +
  `incomingPairingRequestFromJson`; `PairingDecision` enum
  (`accept`/`reject`) + a `wireName`.
- **`domain/daemon_repository.dart`** — add
  `watchIncomingPairingRequest()` and `respondToPairingRequest(...)` to
  the abstract interface.
- **`data/ipc_daemon_repository.dart`** —
  `ReplayChannel<IncomingPairingRequest?>` seeded `null`; route
  `incoming_pairing_request_changed` in `_handleEvent`;
  `respondToPairingRequest` → `_sendCommand('respond_to_pairing_request',
  {'request_id': id, 'decision': decision.wireName})`; add the channel
  to `_failChannelsAwaitingFirstValue` and `dispose`.
- **`data/mock_daemon_repository.dart`** — implement both interface
  members against a `ReplayChannel<IncomingPairingRequest?>` seeded
  `null`. Add a **non-interface** `simulateIncomingPairingRequest({
  String deviceName, HostOs deviceOs})` used by the dev harness and
  widget tests (the mock has no network). `respondToPairingRequest`
  clears the channel and, on `accept`, appends a `Device` and emits
  `devices_changed`; unknown id → throws
  `DaemonCommandException('pairing_request_not_found', ...)`.
- **`state/repository_providers.dart`** (or `ui_providers.dart`) — 
  `incomingPairingRequestProvider =
  StreamProvider<IncomingPairingRequest?>((ref) =>
  ref.watch(daemonRepositoryProvider).watchIncomingPairingRequest())`
  (not `autoDispose` — the listener subscribes for the app's lifetime).
- **New `features/pairing/incoming_pairing_request_dialog.dart`** — a
  stateless dialog widget: title "**{deviceName}** wants to pair", a
  line with the OS and `address`, the `fingerprint` in a monospace
  block with a one-line "Confirm this matches the code on the other
  device" caption, and **Reject** / **Accept** actions. Returns a
  `PairingDecision?` (null = dismissed without choosing → caller treats
  as reject).
- **New `features/pairing/incoming_pairing_request_listener.dart`** — a
  widget mounted high in `_RealApp`'s tree (wrapping `content`). It
  `ref.listen`s `incomingPairingRequestProvider`:
  - transition to non-null while no dialog is open →
    `windowManager.show()`, `.focus()`, a brief
    `setAlwaysOnTop(true)` then `false`, then
    `showDialog(barrierDismissible: true, ...)`.
  - transition to `null` while the dialog is open (timeout / withdrawal
    / already answered) → pop it.
  - dialog result → `repo.respondToPairingRequest(id, decision ??
    PairingDecision.reject)`, swallowing `pairing_request_not_found`.
  - It holds a nullable "currently-shown request id" to avoid stacking
    dialogs and to detect the null transition.
- **`features/harness/dev_harness.dart`** — a button that calls
  `simulateIncomingPairingRequest(...)` so the dialog is QA-able with no
  daemon. Mount the listener in the harness tree too.
- Window bring-to-front reuses the existing `_showWindow()` mechanics in
  `app.dart` (`windowManager.show()` + `.focus()`), factored so the
  listener can call it.

### 1.5 Tests — Track 1

**Daemon**

- `service/mod.rs`:
  - incoming request registers pending state and emits
    `Some(info)` with a well-formed `request_id` + `fingerprint`.
  - `respond_to_pairing_request(accept)` ⇒ decision delivered, device
    persisted with the proven key, state cleared, emits `None`.
  - `respond_to_pairing_request(reject)` ⇒ no device, state cleared.
  - no answer ⇒ auto-reject after `PAIRING_DECISION_TIMEOUT`
    (`tokio::time::pause` + `advance`).
  - a second concurrent inbound request is rejected immediately while
    one is pending.
  - `connected_clients == 0` ⇒ immediate reject, no state change.
  - `respond_to_pairing_request` with an unknown id ⇒
    `PairingRequestNotFound`.
- `tests/pairing_over_channel.rs` — rewrite the end-to-end test:
  - remove the responder-side `start_pairing()` "second press".
  - responder now needs a stand-in "UI": the test subscribes to
    `watch_incoming_request()`, waits for `Some`, then calls
    `service_b.respond_to_pairing_request(id, Accept)`.
  - assert both sides end with the other persisted (keeps the existing
    "proven public key, not None" assertion).
  - add a `Reject` variant: initiator's session ends `Failed`,
    responder persists nothing.
  - `connected_clients` must read as > 0 in this test — either set it
    directly or add a test constructor arg.
- `tests/ipc_protocol.rs` — six initial events in order;
  `respond_to_pairing_request` happy path acks; unknown id errs with
  `pairing_request_not_found`.
- `ipc/server.rs` — initial-event count 5 → 6.
- `core` — `key_fingerprint` output is stable for a fixed key.

**Flutter**

- `test/data/ipc_daemon_repository_test.dart` — routes
  `incoming_pairing_request_changed` onto the stream (incl. the initial
  `null`); `respondToPairingRequest` writes the correct frame; the new
  channel fails closed on transport drop before first value.
- `test/data/mock_daemon_repository_test.dart` — `simulate...` emits;
  `respondToPairingRequest(accept)` adds a device and clears; unknown id
  throws `pairing_request_not_found`.
- `test/features/pairing/incoming_pairing_request_dialog_test.dart` —
  renders name/os/address/fingerprint; Accept/Reject return the right
  `PairingDecision`.
- `test/features/pairing/incoming_pairing_request_listener_test.dart` —
  non-null shows the dialog; choosing Accept calls the repo;
  stream→null pops the dialog; `pairing_request_not_found` is swallowed.
- `test/e2e/daemon_ui_flow_e2e_test.dart` — a flow that drives the mock
  `simulate...` and asserts the dialog appears and Accept adds the
  device.

---

## Track 2 — tray icon opens a native menu

### 2.1 Behavior

`_RealApp` (`flutter/lib/app.dart`):

- **Left-click** (`onTrayIconMouseDown`) and **right-click**
  (`onTrayIconRightMouseDown`) both: rebuild the menu from current state,
  then `trayManager.popUpContextMenu()`. Remove `_toggleWindow` as the
  left-click action. (macOS: menu-on-left-click is the menu-bar-app
  norm; Windows/Linux `tray_manager` supports it too.)
- The menu is **rebuilt on daemon state change** so it is never stale:
  `_RealApp` keeps `ProviderSubscription`s (via a `ConsumerState`
  `ref.listen` in `build`, or a manual `ref.listenManual`) on the
  devices stream and the link-state stream; each change, if
  `_trayReady`, calls `_rebuildTrayMenu()`. Also rebuilt immediately
  before each `popUpContextMenu()` as a backstop.

### 2.2 Menu contents (mirrors `TrayPopover.dc.html`'s IA)

Built from `watchLinkState()` + `watchDevices()` (via
`ref.read(...).valueOrNull`, tolerating not-yet-loaded):

| Item | State | Action |
|---|---|---|
| `Flow — {linkStatusLabel}` | disabled | — |
| — separator — | | |
| `Using: {activeName} ({os})` | disabled; omitted if no active device | — |
| `Switch to {name}` — one per other device whose state is `inactive` or `connected` | enabled | `switchActiveDevice(id)`; on error show a toast (window may be hidden — see 2.3) |
| `{name} — disconnected` — one per unreachable device | disabled | — |
| — separator — | | |
| `Pair New Device…` | enabled | `startPairing()` then show the window on the dashboard section (where the existing pairing UI lives) |
| — separator — | | |
| `Dashboard` | enabled | show window, `AppSection.dashboard` |
| `Settings` | enabled | show window, `AppSection.general` |
| `Quit Flow` | enabled | existing `_quit()` |

`linkStatusLabel` reuses the mapping already in `tray_popover.dart`
(`Connected`, `Connecting…`, `Reconnecting…`, `Disconnected`,
`Connection lost`, `Needs permission`).

### 2.3 Window deep-linking

`AppWindowShell` already takes `initialSection: AppSection`
(`app_window_shell.dart:38`). `_RealApp` currently builds it with no
section. Add `AppSection _pendingSection = AppSection.dashboard` to
`_RealAppState`; menu items set it (`setState`) before `_showWindow()`;
`build` passes `initialSection: _pendingSection`. Because
`AppWindowShell` only reads `initialSection` in `initState`, also give
it a `key: ValueKey(_pendingSection)` so re-selecting a section while the
window is already open remounts it on the right one. ("Pair New Device"
uses `AppSection.dashboard` and relies on `startPairing()` having moved
the session out of `idle`, which the dashboard's pairing view renders.)

Menu-action toasts: `toastProvider` already exists; a toast shown while
the window is hidden is harmless (no overlay) — acceptable for the
interim. Switch errors are the only likely case; log them too.

### 2.4 `_setUpTray` changes

- Replace the static 2-item menu with `_rebuildTrayMenu()` producing the
  table above.
- Keep the existing "degrade gracefully" `try/catch` (no tray plugin in
  `flutter test`) and the `_trayReady` guard.
- Keep "not until onboarding has completed at least once".

### 2.5 Tests — Track 2

`tray_manager` has no test double, so keep this thin and behavioral:

- **Refactor** menu construction into a pure function
  `List<TrayMenuEntry> buildTrayMenu({required DaemonLinkState link,
  required List<Device> devices})` returning a plain data model
  (label, enabled, an `enum TrayAction { switchDevice(id), pair,
  dashboard, settings, quit }`), with `_setUpTray` translating that to
  `tray_manager`'s `Menu`/`MenuItem`. Unit-test the pure function:
  - connected + 1 active + 1 switchable + 1 disconnected ⇒ expected
    labels / enabled flags / actions, in order.
  - no active device ⇒ no "Using:" row.
  - `permissionRequired` ⇒ status label "Needs permission".
- A small dispatch test: `TrayAction.switchDevice(id)` →
  `repo.switchActiveDevice(id)`; `.settings` → window shown with
  `AppSection.general`; `.pair` → `startPairing()` called.
- `flutter analyze` + full `flutter test` stay green.

---

## Delivery / sequencing

Two independent tracks on one branch. Suggested order:

1. **Track 1 daemon** — contract doc + `flow_core` + handshake split +
   service + dispatch + server + `main.rs`, with its Rust tests. Self-
   contained; `pairing_over_channel.rs` is the proof.
2. **Track 1 Flutter** — models, repo, mock, dialog, listener, harness
   hook, tests.
3. **Track 2** — pure `buildTrayMenu` + `_RealApp` wiring + tests.

Each step is TDD (failing test first) per the repo's existing
conventions. `docs/contracts/CHANGELOG.md` and `daemon/README.md` are
updated in step 1.

## Risks / notes

- Removing `is_pairing_window_open` widens what reaches a prompt to *any*
  untrusted inbound connection. Mitigations in this design: one prompt at
  a time (extras auto-rejected), 30s auto-reject, immediate reject with
  no UI. A malicious LAN host can still cause one dialog per 30s while
  the app is open; "block this device" / rate-limiting is the follow-up.
- The fingerprint is display-only assurance — no forced comparison. SAS
  is the follow-up that closes the "first-come-first-served within the
  window" gap the code comments already call out.
- Track 2 is explicitly interim. The glass popover panel from
  `TrayPopover.dc.html` needs either a second OS window
  (`desktop_multi_window` + runner changes) or a single-window
  mode-switch; both were judged out of scope for this pass.
- `peer_addr` threading touches the Bluetooth accept path too — it
  passes `None` there; the dialog shows no address line when empty.
