# Incoming Pairing Prompt + Tray Native Menu — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the daemon's stage-based `is_pairing_window_open` gate with a real Accept/Reject prompt wired end to end (daemon → IPC → Flutter modal → decision → handshake), and make the tray icon open a native menu built from live daemon state instead of toggling the app window.

**Architecture:** The daemon, on any untrusted inbound pairing connection, reads the `PairingRequest`, publishes it on a new `watch` channel, and blocks the handshake on a `oneshot` receiver with a 30s timeout. A new IPC command `respond_to_pairing_request` fires that oneshot. If no IPC client is connected, the daemon rejects immediately. Flutter gains a new state stream, a modal dialog, and a root-mounted listener that brings the window to front. Track 2 is independent: a pure `buildTrayMenu()` function feeds `tray_manager`.

**Tech Stack:** Rust (tokio, tokio-tungstenite, serde), Flutter (flutter_riverpod, window_manager, tray_manager).

**Spec:** `docs/superpowers/specs/2026-08-28-incoming-pairing-prompt-and-tray-menu-design.md`

## Global Constraints

- Contract version bumps to **0.1.3** (additive; precedent: `retry_connection` at 0.1.2).
- New IPC event name: `incoming_pairing_request_changed` (sent on connect, initially `null`; it is the **6th** initial event, last in order).
- New IPC command name: `respond_to_pairing_request`, payload `{ request_id, decision }` where `decision` is `"accept"` or `"reject"` (serde `snake_case` of `PairingDecision`).
- New error code: `pairing_request_not_found`.
- New daemon const: `PAIRING_DECISION_TIMEOUT = Duration::from_secs(30)`.
- Decision resolution mechanism: `tokio::sync::oneshot` + `tokio::time::sleep`. Tests use `tokio::time::pause()` / `advance()`.
- Fingerprint format: SHA-256 of the peer's proven ed25519 public key, first 8 bytes, rendered `"{:02x}{:02x} {:02x}{:02x} {:02x}{:02x} {:02x}{:02x}"` (four space-separated 4-hex-digit groups, e.g. `3f2a 91c4 8d10 6b57`).
- `request_id` format: `format!("ipr-{:032x}", rand::random::<u128>())` (`rand = "0.10"` is already a daemon dep; do **not** add `uuid`).
- `key_fingerprint()` lives in **`flow_daemon`** (add `sha2 = "0.10"` to `daemon/Cargo.toml`), NOT `flow_core` — keep `flow_core` free of crypto deps. The `IncomingPairingRequest` *struct* still lives in `flow_core::pairing` (serde only).
- One pending request at a time: a second concurrent inbound pairing request is rejected immediately while one is awaiting a decision.
- `respond_to_pairing_request` never clears state or emits — `accept_pairing_over` is the single owner of the clear + emit, so there is exactly one `incoming_pairing_request_changed: null` per request.
- TDD: every task writes the failing test first, runs it to see it fail, implements, runs it to see it pass, commits.
- Rust: `cargo test -p <crate>`, `cargo clippy --all-targets -- -D warnings`. Flutter: run from `flutter/`, `flutter test`, `flutter analyze`.
- Commit message trailers (repo convention):
  ```
  Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01BpeLa5fWk5SqaT35y7vHhZ
  ```

---

## File Structure

**flow_core (Rust)**
- `core/src/pairing/mod.rs` — MODIFY: add `IncomingPairingRequest` struct.
- `core/src/error/mod.rs` — MODIFY: add `FlowError::PairingRequestNotFound` + code.

**Contract docs**
- `docs/contracts/daemon-ipc.md` — MODIFY: interface method, event list, command table, error codes.
- `docs/contracts/data-model.md` — MODIFY: `IncomingPairingRequest` type.
- `docs/contracts/CHANGELOG.md` — MODIFY: `## 0.1.3` entry.

**daemon (Rust)**
- `daemon/Cargo.toml` — MODIFY: add `sha2 = "0.10"`.
- `daemon/src/pairing_fingerprint.rs` — CREATE: `key_fingerprint()`.
- `daemon/src/lib.rs` — MODIFY: `pub mod pairing_fingerprint;`.
- `daemon/src/channel/handshake.rs` — MODIFY: split `respond_to_pairing` into `recv_pairing_request` + `send_pairing_decision`; keep a wrapper.
- `daemon/src/service/mod.rs` — MODIFY: `incoming_request` state + `PendingPairingRequest` + `incoming_request_tx` + `watch_incoming_request()` + `PAIRING_DECISION_TIMEOUT` + `connected_clients` + `register_ipc_client()`/`IpcClientGuard` + `connected_client_count()`; rewrite `accept_pairing_over` (new `peer_addr` param); add `respond_to_pairing_request()`; delete `is_pairing_window_open` + its 2 tests.
- `daemon/src/ipc/dispatch.rs` — MODIFY: `respond_to_pairing_request` arm + payload struct.
- `daemon/src/ipc/server.rs` — MODIFY: register client guard; 6th initial event; select arm; bump "five" → "six" test.
- `daemon/src/main.rs` — MODIFY: capture `stream.peer_addr()`, thread through `accept_incoming_peer_channel`.
- `daemon/README.md` — MODIFY: rewrite the "only while ... pairing session open" paragraph (~line 289).
- `daemon/tests/pairing_over_channel.rs` — MODIFY: rewrite the end-to-end test (drive `respond_to_pairing_request`).
- `daemon/tests/ipc_protocol.rs` — MODIFY: 6 initial events; new command ack/err.

**Flutter**
- `flutter/lib/domain/pairing.dart` — MODIFY: `IncomingPairingRequest`, `PairingDecision`, `incomingPairingRequestFromJson`.
- `flutter/lib/domain/daemon_repository.dart` — MODIFY: 2 new interface members.
- `flutter/lib/data/ipc_daemon_repository.dart` — MODIFY: new `ReplayChannel`, event routing, command, dispose/fail lists.
- `flutter/lib/data/mock_daemon_repository.dart` — MODIFY: implement + `simulateIncomingPairingRequest()`.
- `flutter/lib/state/repository_providers.dart` — MODIFY: `incomingPairingRequestProvider`.
- `flutter/lib/features/pairing/incoming_pairing_request_dialog.dart` — CREATE.
- `flutter/lib/features/pairing/incoming_pairing_request_listener.dart` — CREATE.
- `flutter/lib/features/tray/tray_menu.dart` — CREATE: `buildTrayMenu()` + `TrayMenuEntry` + `TrayAction`.
- `flutter/lib/app.dart` — MODIFY: mount listener; factor `showMainWindow()`; tray menu wiring; `_pendingSection`.
- `flutter/lib/features/harness/dev_harness.dart` — MODIFY: "Simulate incoming pairing request" button; mount the listener.

**Flutter tests**
- `flutter/test/data/mock_daemon_repository_test.dart` — MODIFY.
- `flutter/test/data/ipc_daemon_repository_test.dart` — MODIFY.
- `flutter/test/features/pairing/incoming_pairing_request_dialog_test.dart` — CREATE.
- `flutter/test/features/pairing/incoming_pairing_request_listener_test.dart` — CREATE.
- `flutter/test/features/tray/tray_menu_test.dart` — CREATE.
- `flutter/test/e2e/daemon_ui_flow_e2e_test.dart` — MODIFY.

---

## Phase A — flow_core + contract docs

### Task 1: `IncomingPairingRequest` struct in `flow_core`

**Files:**
- Modify: `core/src/pairing/mod.rs`
- Test: `core/src/pairing/mod.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `flow_core::pairing::IncomingPairingRequest { request_id: String, device_name: String, device_os: HostOs, fingerprint: String, address: String }` — `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`, `#[serde(rename_all = "snake_case")]`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `core/src/pairing/mod.rs`:

```rust
#[test]
fn incoming_pairing_request_serializes_snake_case() {
    let req = IncomingPairingRequest {
        request_id: "ipr-abc".to_string(),
        device_name: "Abhishek's Windows".to_string(),
        device_os: HostOs::Windows,
        fingerprint: "3f2a 91c4 8d10 6b57".to_string(),
        address: "192.168.0.103".to_string(),
    };
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["request_id"], "ipr-abc");
    assert_eq!(json["device_os"], "windows");
    assert_eq!(json["fingerprint"], "3f2a 91c4 8d10 6b57");
    let round: IncomingPairingRequest = serde_json::from_value(json).unwrap();
    assert_eq!(round, req);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flow-core incoming_pairing_request_serializes_snake_case`
Expected: FAIL — `cannot find type IncomingPairingRequest`.

- [ ] **Step 3: Write minimal implementation**

Add after the `PairingCandidate` struct in `core/src/pairing/mod.rs`:

```rust
/// An incoming pairing request awaiting the local user's decision,
/// surfaced to the UI as `incoming_pairing_request_changed`
/// (`docs/contracts/daemon-ipc.md`). All fields except `request_id` and
/// `fingerprint` are self-reported by the peer and are display-only;
/// `fingerprint` is a short hash of the peer's *proven* identity key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IncomingPairingRequest {
    /// Opaque, daemon-generated; echoed back in `respond_to_pairing_request`.
    pub request_id: String,
    pub device_name: String,
    pub device_os: HostOs,
    /// e.g. `"3f2a 91c4 8d10 6b57"` — first 8 bytes of SHA-256 of the
    /// peer's proven ed25519 public key.
    pub fingerprint: String,
    /// Peer source IP, display only; empty when unknown (e.g. Bluetooth).
    pub address: String,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p flow-core incoming_pairing_request_serializes_snake_case`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add core/src/pairing/mod.rs
git commit -m "feat(core): add IncomingPairingRequest wire type"
```

---

### Task 2: `FlowError::PairingRequestNotFound`

**Files:**
- Modify: `core/src/error/mod.rs`
- Test: `core/src/error/mod.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `FlowError::PairingRequestNotFound` variant; `.code()` returns `"pairing_request_not_found"`.

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)] mod tests` block of `core/src/error/mod.rs`, inside `every_variant_reports_its_contract_code` (or as a new `#[test]`), add:

```rust
assert_eq!(
    FlowError::PairingRequestNotFound.code(),
    "pairing_request_not_found"
);
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flow-core every_variant_reports_its_contract_code`
Expected: FAIL — `no variant named PairingRequestNotFound`.

- [ ] **Step 3: Write minimal implementation**

Add the variant to `enum FlowError` (after `LinkNotRecoverable`):

```rust
    #[error("no pairing request is awaiting a decision for that id")]
    PairingRequestNotFound,
```

Add the arm to `code()`:

```rust
            FlowError::PairingRequestNotFound => "pairing_request_not_found",
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p flow-core`
Expected: PASS (whole crate).

- [ ] **Step 5: Commit**

```bash
git add core/src/error/mod.rs
git commit -m "feat(core): add PairingRequestNotFound error code"
```

---

### Task 3: Contract documentation for 0.1.3

**Files:**
- Modify: `docs/contracts/daemon-ipc.md`
- Modify: `docs/contracts/data-model.md`
- Modify: `docs/contracts/CHANGELOG.md`

**Interfaces:**
- Produces: the authoritative shapes Tasks 8, 9, 11, 12, 13 implement against. No code; the gate is a contract-consistency read-through.

- [ ] **Step 1: `data-model.md` — add the type**

After the `PairingSession` block in `docs/contracts/data-model.md`, add:

```dart
class IncomingPairingRequest {
  final String requestId;   // opaque, daemon-generated
  final String deviceName;  // self-reported by the peer; display only
  final HostOs deviceOs;    // self-reported by the peer
  final String fingerprint; // "3f2a 91c4 8d10 6b57" — short hash of the
                            // peer's PROVEN ed25519 public key
  final String address;     // peer source IP, display only ("" if unknown)
}
```

```json
{
  "request_id": "ipr-9f2c1a...",
  "device_name": "Abhishek's Windows",
  "device_os": "windows",
  "fingerprint": "3f2a 91c4 8d10 6b57",
  "address": "192.168.0.103"
}
```

- [ ] **Step 2: `daemon-ipc.md` — interface, events, command, errors**

  1. Header line: append to the version history parenthetical: `, incoming pairing prompt added in **0.1.3**`.
  2. In the `abstract class DaemonRepository` block, under the `watch*` streams add:
     ```dart
     Stream<IncomingPairingRequest?> watchIncomingPairingRequest();
     ```
     and under the commands add:
     ```dart
     Future<void> respondToPairingRequest(String requestId, PairingDecision decision);
     ```
  3. In the "Every `watch*` stream corresponds to one `event` name" bullet, add `incoming_pairing_request_changed` to the list, and note it is sent last on connect (initially `null`).
  4. In the error-codes bullet, add `pairing_request_not_found` to the list.
  5. In the Commands table add a row:
     ```
     | `respond_to_pairing_request` | `{ request_id, decision: "accept" | "reject" }` | A request with `request_id` is currently pending. | Unblocks the daemon's pairing handshake with that decision and clears the pending request (emits `incoming_pairing_request_changed: null`). On `accept`, the peer is added and appears via `devices_changed`. Error `pairing_request_not_found` if no such request is pending (already answered / timed out / withdrawn). |
     ```
  6. Add a short subsection after the Pairing state machine:
     ```markdown
     ### Incoming pairing requests

     Independent of `PairingSession` (which models only the *initiating*
     side). When another device connects to this daemon's peer listener
     and is not already trusted, the daemon publishes an
     `IncomingPairingRequest` on `watchIncomingPairingRequest()` and
     waits up to ~30s for `respond_to_pairing_request`. No connected UI,
     no answer within the timeout, or a second request arriving while one
     is pending ⇒ the daemon rejects that request and the initiator sees
     a normal pairing failure. Exactly one request is surfaced at a time;
     the stream is `null` whenever none is pending.
     ```

- [ ] **Step 3: `CHANGELOG.md` — new entry at the top**

```markdown
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
```

- [ ] **Step 4: Consistency read-through**

Re-read all three edits together. Confirm: field names match Task 1's struct (`request_id`/`device_name`/`device_os`/`fingerprint`/`address`); the event name is exactly `incoming_pairing_request_changed` everywhere; the command name is exactly `respond_to_pairing_request`; the error code is exactly `pairing_request_not_found`.

- [ ] **Step 5: Commit**

```bash
git add docs/contracts/
git commit -m "docs(contract): 0.1.3 — incoming pairing prompt stream + command"
```

---

## Phase B — daemon

### Task 4: Split the pairing handshake responder

**Files:**
- Modify: `daemon/src/channel/handshake.rs`
- Test: `daemon/src/channel/handshake.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: `flow_core::channel::{Channel, ChannelError, ChannelMessage, PairingWireMessage}`, `flow_core::pairing::{PairingDecision, PairingRequest}`.
- Produces:
  - `pub async fn recv_pairing_request(channel: &mut dyn Channel) -> Result<PairingRequest, ChannelError>`
  - `pub async fn send_pairing_decision(channel: &mut dyn Channel, decision: PairingDecision) -> Result<(), ChannelError>`
  - `pub async fn respond_to_pairing(channel, decide: impl FnOnce(&PairingRequest) -> PairingDecision) -> Result<(PairingRequest, PairingDecision), ChannelError>` — unchanged signature, now implemented via the two above.

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `daemon/src/channel/handshake.rs`:

```rust
#[tokio::test]
async fn recv_then_send_halves_round_trip_like_respond_to_pairing() {
    let (mut initiator, mut responder) = connected_pair().await;
    let request = a_request();

    let responder_task = tokio::spawn(async move {
        let got = recv_pairing_request(&mut responder).await.expect("recv");
        send_pairing_decision(&mut responder, PairingDecision::Accept)
            .await
            .expect("send");
        got
    });

    let decision = request_pairing(&mut initiator, request.clone())
        .await
        .expect("decision");
    assert_eq!(decision, PairingDecision::Accept);
    assert_eq!(responder_task.await.expect("task"), request);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flow-daemon recv_then_send_halves_round_trip`
Expected: FAIL — `cannot find function recv_pairing_request`.

- [ ] **Step 3: Write minimal implementation**

Replace the body of `respond_to_pairing` and add the two functions:

```rust
/// Responder side, part 1: waits for the peer's `PairingRequest`.
/// A non-pairing frame received while waiting is ignored (this
/// connection may carry other traffic once input streaming shares it).
pub async fn recv_pairing_request(
    channel: &mut dyn Channel,
) -> Result<PairingRequest, ChannelError> {
    loop {
        if let ChannelMessage::Pairing(PairingWireMessage::Request(request)) =
            channel.recv().await?
        {
            return Ok(request);
        }
    }
}

/// Responder side, part 2: sends the decision back to the initiator.
pub async fn send_pairing_decision(
    channel: &mut dyn Channel,
    decision: PairingDecision,
) -> Result<(), ChannelError> {
    channel
        .send(ChannelMessage::Pairing(PairingWireMessage::Decision(
            decision,
        )))
        .await
}

/// Responder side: waits for the request, decides synchronously via
/// `decide`, sends the decision. Retained for callers that don't need to
/// await between receiving and deciding (this module's own tests).
pub async fn respond_to_pairing(
    channel: &mut dyn Channel,
    decide: impl FnOnce(&PairingRequest) -> PairingDecision,
) -> Result<(PairingRequest, PairingDecision), ChannelError> {
    let request = recv_pairing_request(channel).await?;
    let decision = decide(&request);
    send_pairing_decision(channel, decision).await?;
    Ok((request, decision))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flow-daemon --lib channel::handshake`
Expected: PASS (new test + all pre-existing handshake tests).

- [ ] **Step 5: Commit**

```bash
git add daemon/src/channel/handshake.rs
git commit -m "refactor(daemon): split respond_to_pairing into recv/send halves"
```

---

### Task 5: IPC client-connection counter

**Files:**
- Modify: `daemon/src/service/mod.rs` (struct field, constructor, methods)
- Modify: `daemon/src/ipc/server.rs` (acquire the guard on a live connection)
- Test: `daemon/src/ipc/server.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces:
  - `DaemonService::connected_client_count(&self) -> usize`
  - `DaemonService::register_ipc_client(&self) -> IpcClientGuard` — increments now, decrements on `Drop`.
  - `pub struct IpcClientGuard` (in `service` module, re-exported as needed).

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `daemon/src/ipc/server.rs`:

```rust
#[tokio::test]
async fn a_live_connection_is_counted_and_uncounted_on_drop() {
    let storage = Storage::open_in_memory().await.expect("open db");
    let service = Arc::new(DaemonService::new_seeded_for_test(storage).await);
    assert_eq!(service.connected_client_count(), 0);
    {
        let _guard = service.register_ipc_client();
        assert_eq!(service.connected_client_count(), 1);
    }
    assert_eq!(service.connected_client_count(), 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flow-daemon a_live_connection_is_counted_and_uncounted_on_drop`
Expected: FAIL — `no method named register_ipc_client`.

- [ ] **Step 3: Write minimal implementation**

In `daemon/src/service/mod.rs`:

- Add imports near the top: `use std::sync::atomic::{AtomicUsize, Ordering};` (keep existing `use std::sync::Arc;`).
- Add a field to `struct DaemonService`:
  ```rust
      /// Number of IPC clients (Flutter UIs) currently connected. Zero
      /// means an incoming pairing request has no one to prompt and is
      /// rejected outright — see `accept_pairing_over`.
      connected_clients: Arc<AtomicUsize>,
  ```
- In `from_state`, add to the struct literal: `connected_clients: Arc::new(AtomicUsize::new(0)),`.
- Add methods in the `impl DaemonService` block (near `watch_*`):
  ```rust
  /// How many IPC clients are connected right now.
  pub fn connected_client_count(&self) -> usize {
      self.connected_clients.load(Ordering::Relaxed)
  }

  /// Registers one live IPC client for the lifetime of the returned
  /// guard. `ipc::server::handle_connection` holds it for the duration
  /// of a connection; tests hold it to simulate a connected UI.
  pub fn register_ipc_client(&self) -> IpcClientGuard {
      self.connected_clients.fetch_add(1, Ordering::Relaxed);
      IpcClientGuard {
          counter: Arc::clone(&self.connected_clients),
      }
  }
  ```
- Add at module level (after the `impl DaemonService` block):
  ```rust
  /// Decrements the connected-client count when dropped.
  pub struct IpcClientGuard {
      counter: Arc<AtomicUsize>,
  }

  impl Drop for IpcClientGuard {
      fn drop(&mut self) {
          self.counter.fetch_sub(1, Ordering::Relaxed);
      }
  }
  ```

In `daemon/src/ipc/server.rs`, inside `handle_connection`, immediately after `tracing::debug!("ipc connection established");` (the point where the handshake has succeeded):

```rust
    let _client_guard = service.register_ipc_client();
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flow-daemon --lib ipc::server`
Expected: PASS (new test + all pre-existing server tests).

- [ ] **Step 5: Commit**

```bash
git add daemon/src/service/mod.rs daemon/src/ipc/server.rs
git commit -m "feat(daemon): count connected IPC clients"
```

---

### Task 6: `key_fingerprint()` helper

**Files:**
- Modify: `daemon/Cargo.toml` (add `sha2 = "0.10"`)
- Create: `daemon/src/pairing_fingerprint.rs`
- Modify: `daemon/src/lib.rs` (declare the module)
- Test: `daemon/src/pairing_fingerprint.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `flow_daemon::pairing_fingerprint::key_fingerprint(public_key: &[u8]) -> String`.

- [ ] **Step 1: Write the failing test**

Create `daemon/src/pairing_fingerprint.rs` with only the test first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_and_shaped_for_a_fixed_key() {
        let key = [0u8; 32];
        let fp = key_fingerprint(&key);
        // 4 groups of 4 lowercase hex digits, single-space separated.
        assert_eq!(fp.len(), 19);
        assert_eq!(fp.split(' ').count(), 4);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit() || c == ' '));
        // Deterministic: SHA-256 of 32 zero bytes starts 66687aad...
        assert_eq!(fp, "6668 7aad f862 bd77");
    }

    #[test]
    fn different_keys_give_different_fingerprints() {
        assert_ne!(key_fingerprint(&[0u8; 32]), key_fingerprint(&[1u8; 32]));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

First add `sha2 = "0.10"` under `[dependencies]` in `daemon/Cargo.toml`, and `pub mod pairing_fingerprint;` to `daemon/src/lib.rs` (alphabetical with the other `pub mod` lines).

Run: `cargo test -p flow-daemon --lib pairing_fingerprint`
Expected: FAIL — `cannot find function key_fingerprint`.

- [ ] **Step 3: Write minimal implementation**

Prepend to `daemon/src/pairing_fingerprint.rs` (above the test module):

```rust
//! A short, human-comparable fingerprint of a peer's proven identity
//! key, shown in the incoming-pairing prompt so the user has something
//! tied to the *authenticated* key rather than only the peer's
//! self-reported name.

use sha2::{Digest, Sha256};

/// First 8 bytes of SHA-256(`public_key`), as four space-separated
/// 4-hex-digit groups, e.g. `"3f2a 91c4 8d10 6b57"`.
pub fn key_fingerprint(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    format!(
        "{:02x}{:02x} {:02x}{:02x} {:02x}{:02x} {:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    )
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flow-daemon --lib pairing_fingerprint`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add daemon/Cargo.toml Cargo.lock daemon/src/pairing_fingerprint.rs daemon/src/lib.rs
git commit -m "feat(daemon): key_fingerprint helper for the pairing prompt"
```

---

### Task 7: Prompt-driven `accept_pairing_over` + `respond_to_pairing_request`

**Files:**
- Modify: `daemon/src/service/mod.rs`
- Test: `daemon/src/service/mod.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: Task 1 `IncomingPairingRequest`; Task 2 `FlowError::PairingRequestNotFound`; Task 4 `handshake::{recv_pairing_request, send_pairing_decision}`; Task 5 `register_ipc_client` / `connected_client_count`; Task 6 `key_fingerprint`.
- Produces:
  - `DaemonService::watch_incoming_request(&self) -> tokio::sync::watch::Receiver<Option<IncomingPairingRequest>>`
  - `DaemonService::respond_to_pairing_request(&self, request_id: &str, decision: PairingDecision) -> Result<(), FlowError>`
  - `accept_pairing_over(&self, channel: &mut dyn Channel, peer_public_key: Vec<u8>, peer_addr: Option<std::net::SocketAddr>) -> Result<(), ChannelError>` — **new third param**.
  - `accept_pairing_request(&self, channel: Box<dyn Channel>, peer_addr: Option<std::net::SocketAddr>)` and `accept_incoming_peer_channel(&self, channel: Box<dyn Channel>, peer_addr: Option<std::net::SocketAddr>)` — **new second param**.
- Removes: `is_pairing_window_open` and its two tests (`an_incoming_pairing_request_is_rejected_when_no_pairing_window_is_open`, `an_incoming_pairing_request_is_accepted_while_a_pairing_window_is_open`).

- [ ] **Step 1: Write the failing tests**

Add a `#[cfg(test)]` test module section in `daemon/src/service/mod.rs` (near the existing pairing tests). Use `tokio::time` pause where noted.

```rust
#[tokio::test]
async fn an_incoming_request_is_published_and_accepted_by_respond() {
    use crate::channel::test_support::channel_pair; // in-memory Channel pair
    use flow_core::pairing::{PairingDecision, PairingRequest};
    use flow_core::device::HostOs;

    let storage = Storage::open_in_memory().await.expect("db");
    let service = DaemonService::new_seeded_for_test(storage).await;
    let _client = service.register_ipc_client(); // pretend a UI is connected

    let (mut initiator, mut responder) = channel_pair();
    let peer_key = vec![9u8; 32];

    let mut requests = service.watch_incoming_request();
    let _ = requests.borrow_and_update();

    let svc = service.clone();
    let acceptor = tokio::spawn(async move {
        svc.accept_pairing_over(&mut responder, peer_key, None).await
    });

    // Initiator sends its request frame.
    crate::channel::handshake::request_pairing(
        &mut initiator,
        PairingRequest {
            device_name: "Windows Box".to_string(),
            device_os: HostOs::Windows,
            address: String::new(),
        },
    );
    // (request_pairing is awaited below via the decision; drive it concurrently)

    requests.changed().await.expect("published");
    let pending = requests.borrow_and_update().clone().expect("some");
    assert_eq!(pending.device_name, "Windows Box");
    assert!(pending.request_id.starts_with("ipr-"));

    service
        .respond_to_pairing_request(&pending.request_id, PairingDecision::Accept)
        .await
        .expect("respond accept");

    acceptor.await.expect("join").expect("accept ok");

    // Peer persisted, keyed by proven key; stream cleared.
    assert!(service
        .watch_devices()
        .borrow()
        .iter()
        .any(|d| d.name == "Windows Box"));
    assert!(requests.borrow_and_update().is_none());
}
```

> NOTE for the implementer: the existing repo already has an in-memory
> `Channel` test double used by `channel::handshake` tests and
> `pairing_over_channel.rs`. Locate it (search for `ChannelPair` /
> `channel_pair` / the `G1` abstraction tests) and use that exact
> helper; adjust the `use` path above to match. The initiator side must
> be driven concurrently with the responder — spawn `request_pairing`
> in its own task and `await` its `JoinHandle` for the decision, mirror
> the pattern in `daemon/tests/pairing_over_channel.rs`.

Add the remaining scenarios as separate `#[tokio::test]`s:

```rust
// reject path: no device persisted, initiator sees Reject
async fn respond_reject_persists_nothing() { /* as above, PairingDecision::Reject; assert no device with that name; assert requests cleared */ }

// timeout: with tokio::time::paused, advance past PAIRING_DECISION_TIMEOUT
async fn no_answer_auto_rejects_after_timeout() {
    // tokio::test(start_paused = true)
    // spawn accept_pairing_over; wait for watch to publish Some;
    // tokio::time::advance(PAIRING_DECISION_TIMEOUT + Duration::from_millis(1)).await;
    // acceptor completes Ok; requests cleared; no device persisted
}

// no UI: register no client; accept_pairing_over returns Ok, publishes nothing, persists nothing
async fn no_connected_ui_rejects_immediately() { /* do NOT call register_ipc_client */ }

// concurrency: one pending, a second accept_pairing_over is rejected immediately
async fn a_second_request_while_one_is_pending_is_rejected() { /* start two acceptors; only one publishes; second returns Ok fast, no second publish */ }

// unknown id
async fn respond_with_unknown_id_errs() {
    let storage = Storage::open_in_memory().await.expect("db");
    let service = DaemonService::new_seeded_for_test(storage).await;
    let err = service
        .respond_to_pairing_request("ipr-nope", flow_core::pairing::PairingDecision::Accept)
        .await
        .unwrap_err();
    assert_eq!(err, flow_core::error::FlowError::PairingRequestNotFound);
}
```

Also: **delete** the two `is_pairing_window_open` tests named in the Interfaces block.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flow-daemon --lib service::`
Expected: FAIL — `accept_pairing_over` arity mismatch / `no method named watch_incoming_request` / `respond_to_pairing_request`.

- [ ] **Step 3: Write the implementation**

In `daemon/src/service/mod.rs`:

1. Imports: add `use std::net::SocketAddr;`, `use tokio::sync::oneshot;`, `use flow_core::pairing::IncomingPairingRequest;`, `use crate::pairing_fingerprint::key_fingerprint;`. Ensure `use crate::channel::handshake;` (or `handshake::{recv_pairing_request, send_pairing_decision}`) is present.

2. Const near the other `PAIRING_*`:
   ```rust
   /// How long an incoming pairing request waits for the local user's
   /// Accept/Reject before the daemon rejects it on their behalf.
   const PAIRING_DECISION_TIMEOUT: Duration = Duration::from_secs(30);
   ```

3. `ServiceState`: add a field
   ```rust
       /// The single incoming pairing request currently awaiting a
       /// decision, if any. `None` whenever nothing is pending.
       pub incoming_request: Option<PendingPairingRequest>,
   ```
   and a module-level struct:
   ```rust
   /// An incoming pairing request the daemon has surfaced to the UI and
   /// is blocking a handshake on. `respond_to_pairing_request` fires
   /// `responder`; `accept_pairing_over` owns clearing this slot.
   pub struct PendingPairingRequest {
       pub info: IncomingPairingRequest,
       responder: oneshot::Sender<PairingDecision>,
   }
   ```
   Initialize `incoming_request: None` in **both** `from_storage` and `seeded_for_test` struct literals.

4. `DaemonService`: add field `incoming_request_tx: watch::Sender<Option<IncomingPairingRequest>>,`. In `from_state`:
   ```rust
   let (incoming_request_tx, _) = watch::channel(None);
   ```
   and add `incoming_request_tx,` to the struct literal.
   Add the accessor next to the other `watch_*`:
   ```rust
   pub fn watch_incoming_request(
       &self,
   ) -> watch::Receiver<Option<IncomingPairingRequest>> {
       self.incoming_request_tx.subscribe()
   }
   ```

5. **Delete** `pub async fn is_pairing_window_open(&self) -> bool { ... }` and its doc comment.

6. Rewrite `accept_pairing_over`:
   ```rust
   async fn accept_pairing_over(
       &self,
       channel: &mut dyn Channel,
       peer_public_key: Vec<u8>,
       peer_addr: Option<SocketAddr>,
   ) -> Result<(), ChannelError> {
       let request = handshake::recv_pairing_request(channel).await?;

       // No UI connected ⇒ nobody can consent. Reject outright.
       if self.connected_client_count() == 0 {
           tracing::info!(
               "declined an incoming pairing request: no UI connected to prompt"
           );
           return handshake::send_pairing_decision(channel, PairingDecision::Reject).await;
       }

       let info = IncomingPairingRequest {
           request_id: format!("ipr-{:032x}", rand::random::<u128>()),
           device_name: request.device_name.clone(),
           device_os: request.device_os,
           fingerprint: key_fingerprint(&peer_public_key),
           address: peer_addr
               .map(|a| a.ip().to_string())
               .unwrap_or_default(),
       };

       let (tx, rx) = oneshot::channel();
       {
           let mut state = self.state.write().await;
           if state.incoming_request.is_some() {
               tracing::info!(
                   "declined an incoming pairing request: another request is awaiting a decision"
               );
               drop(state);
               return handshake::send_pairing_decision(channel, PairingDecision::Reject).await;
           }
           state.incoming_request = Some(PendingPairingRequest {
               info: info.clone(),
               responder: tx,
           });
       }
       self.incoming_request_tx.send_replace(Some(info));

       let decision = tokio::select! {
           received = rx => received.unwrap_or(PairingDecision::Reject),
           _ = tokio::time::sleep(PAIRING_DECISION_TIMEOUT) => {
               tracing::info!("incoming pairing request timed out with no decision");
               PairingDecision::Reject
           }
       };

       {
           let mut state = self.state.write().await;
           state.incoming_request = None;
       }
       self.incoming_request_tx.send_replace(None);

       handshake::send_pairing_decision(channel, decision).await?;
       if decision != PairingDecision::Accept {
           return Ok(());
       }

       // (unchanged) persist the peer keyed by its proven public key
       let device_id = device_id_from_public_key(&peer_public_key);
       let device = Device {
           id: device_id.clone(),
           name: request.device_name,
           os: request.device_os,
           state: DeviceState::Inactive,
           last_seen: Utc::now(),
       };
       {
           let mut state = self.state.write().await;
           state.devices.insert(device_id.clone(), device.clone());
       }
       DeviceRepo::new(self.storage.clone())
           .upsert(DeviceRecord {
               device,
               public_key: Some(peer_public_key),
               removable: true,
           })
           .await;
       self.emit_devices().await;
       Ok(())
   }
   ```

7. Add `respond_to_pairing_request`:
   ```rust
   /// Delivers the local user's Accept/Reject for a pending incoming
   /// pairing request. `accept_pairing_over` owns clearing the pending
   /// slot and emitting `incoming_pairing_request_changed: null`, so
   /// this only routes the decision.
   pub async fn respond_to_pairing_request(
       &self,
       request_id: &str,
       decision: PairingDecision,
   ) -> Result<(), FlowError> {
       let responder = {
           let mut state = self.state.write().await;
           match &state.incoming_request {
               Some(pending) if pending.info.request_id == request_id => {
                   state.incoming_request.take().map(|p| p.responder)
               }
               _ => None,
           }
       };
       match responder {
           Some(tx) => {
               // Send failure ⇒ the acceptor already timed out; harmless.
               let _ = tx.send(decision);
               Ok(())
           }
           None => Err(FlowError::PairingRequestNotFound),
       }
   }
   ```

8. Update the two callers' signatures to thread `peer_addr`:
   - `accept_pairing_request(&self, channel: Box<dyn Channel>, peer_addr: Option<SocketAddr>)` — pass `peer_addr` into `accept_pairing_over`.
   - `accept_incoming_peer_channel(&self, channel: Box<dyn Channel>, peer_addr: Option<SocketAddr>)` — pass `peer_addr` into its `accept_pairing_over` call.
   (main.rs call sites are fixed in Task 9; the crate won't build standalone until then — that's expected, the lib tests in this task still compile because they call `accept_pairing_over` directly. If `cargo test -p flow-daemon --lib` fails to build on the integration targets, use `cargo test -p flow-daemon --lib service::` which builds only the lib.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flow-daemon --lib service::`
Expected: PASS — all new scenarios; the two deleted window tests are gone.

- [ ] **Step 5: Commit**

```bash
git add daemon/src/service/mod.rs
git commit -m "feat(daemon): prompt-driven incoming pairing (replaces pairing window)"
```

---

### Task 8: `respond_to_pairing_request` IPC command

**Files:**
- Modify: `daemon/src/ipc/dispatch.rs`
- Test: `daemon/src/ipc/dispatch.rs` (inline `#[cfg(test)]`) or `daemon/tests/ipc_protocol.rs` — use whichever the existing command tests use; dispatch.rs has an inline test module (see its `start_pairing` test around line 258).

**Interfaces:**
- Consumes: Task 7 `DaemonService::respond_to_pairing_request`.
- Produces: dispatch handles command `"respond_to_pairing_request"` with payload `{ request_id: String, decision: PairingDecision }`.

- [ ] **Step 1: Write the failing test**

In `daemon/src/ipc/dispatch.rs`'s `#[cfg(test)] mod tests`:

```rust
#[tokio::test]
async fn respond_to_pairing_request_with_unknown_id_errs_with_contract_code() {
    let service = test_service().await; // match the helper the other tests use
    let resp = dispatch(
        &service,
        IpcRequest {
            id: "req-1".to_string(),
            command: "respond_to_pairing_request".to_string(),
            payload: serde_json::json!({ "request_id": "ipr-x", "decision": "accept" }),
        },
    )
    .await;
    match resp {
        IpcResponse::Err { error, .. } => {
            assert_eq!(error.code, "pairing_request_not_found");
        }
        other => panic!("expected Err, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flow-daemon --lib ipc::dispatch::tests::respond_to_pairing_request_with_unknown_id`
Expected: FAIL — dispatch returns `unknown_command`.

- [ ] **Step 3: Write minimal implementation**

Add the arm in `handle` (after `"cancel_pairing"`):

```rust
        "respond_to_pairing_request" => {
            let args: RespondToPairingRequestPayload = parse_payload(payload)?;
            service
                .respond_to_pairing_request(&args.request_id, args.decision)
                .await
                .map_err(ErrorPayload::from)
        }
```

Add the payload struct near the other payload structs at the bottom of the file:

```rust
#[derive(serde::Deserialize)]
struct RespondToPairingRequestPayload {
    request_id: String,
    decision: flow_core::pairing::PairingDecision,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p flow-daemon --lib ipc::dispatch`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add daemon/src/ipc/dispatch.rs
git commit -m "feat(daemon): respond_to_pairing_request IPC command"
```

---

### Task 9: Emit `incoming_pairing_request_changed`; thread `peer_addr` in main.rs; README

**Files:**
- Modify: `daemon/src/ipc/server.rs` (6th initial event + select arm + "six" test)
- Modify: `daemon/src/main.rs` (capture + pass `peer_addr`)
- Modify: `daemon/README.md` (~line 289 paragraph)
- Test: `daemon/src/ipc/server.rs` inline test; `daemon/tests/ipc_protocol.rs`

**Interfaces:**
- Consumes: Task 7 `watch_incoming_request`; Task 7 new `accept_incoming_peer_channel` signature.
- Produces: on connect, six initial events ending with `incoming_pairing_request_changed`; subsequent changes forwarded.

- [ ] **Step 1: Write the failing tests**

In `daemon/src/ipc/server.rs`, update `a_new_connection_receives_exactly_five_initial_events_in_order`: rename to `..._six_initial_events_in_order`, append `"incoming_pairing_request_changed"` to `expected_order`, and change the loop count / comment accordingly.

In `daemon/tests/ipc_protocol.rs`, find the array of expected initial event names (it lists `"pairing_session_changed"` etc.) and append `"incoming_pairing_request_changed"`; bump any hard-coded count.

Add to `daemon/tests/ipc_protocol.rs` a command round-trip:

```rust
// after connecting `ws` and draining the six initial events:
send_command(&mut ws, "req-x", "respond_to_pairing_request",
    json!({ "request_id": "ipr-none", "decision": "reject" })).await;
let reply = next_reply(&mut ws).await; // match the helper used in this file
assert_eq!(reply["id"], "req-x");
assert_eq!(reply["ok"], false);
assert_eq!(reply["error"]["code"], "pairing_request_not_found");
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flow-daemon --lib ipc::server` and `cargo test -p flow-daemon --test ipc_protocol`
Expected: FAIL — only five events sent; unknown command.

- [ ] **Step 3: Write the implementation**

`daemon/src/ipc/server.rs`:

- Add subscription with the others: `let mut incoming_request_rx = service.watch_incoming_request();`
- After the `permission` initial-event block, add a sixth:
  ```rust
  let incoming_request = incoming_request_rx.borrow_and_update().clone();
  if send_event(&mut sink, "incoming_pairing_request_changed", &incoming_request)
      .await
      .is_err()
  {
      return;
  }
  ```
- Add a `select!` arm in the loop:
  ```rust
  changed = incoming_request_rx.changed() => {
      if changed.is_err() {
          break;
      }
      let value = incoming_request_rx.borrow_and_update().clone();
      if send_event(&mut sink, "incoming_pairing_request_changed", &value).await.is_err() {
          break;
      }
  }
  ```
- Update the `handle_connection` doc comment: "five initial state-push events" → "six".

`daemon/src/main.rs`:

- In `handle_incoming_peer_stream`, capture the address before the stream is consumed:
  ```rust
  async fn handle_incoming_peer_stream(
      service: Arc<DaemonService>,
      stream: TcpStream,
      connected_peers: ConnectedPeers,
  ) {
      let peer_addr = stream.peer_addr().ok();
      let channel: Box<dyn Channel> = match TcpChannel::accept(stream).await {
          Ok(channel) => Box::new(channel),
          Err(err) => {
              tracing::debug!("peer connection failed the WebSocket handshake: {err}");
              return;
          }
      };
      match service.accept_incoming_peer_channel(channel, peer_addr).await {
          // ...unchanged arms...
      }
  }
  ```
- If any other call site of `accept_incoming_peer_channel` / `accept_pairing_request` exists (grep), pass `None`.

`daemon/README.md` (~line 289): replace the paragraph beginning
"`DaemonService::accept_pairing_request` accepts an incoming pairing
request **only while this daemon's own user has a pairing session
open**..." with:

```markdown
`DaemonService::accept_pairing_request` / `accept_incoming_peer_channel`
surface every untrusted inbound pairing attempt to the connected UI as an
`IncomingPairingRequest` (`watch_incoming_request`, event
`incoming_pairing_request_changed`) and block the handshake until the
user answers via the `respond_to_pairing_request` IPC command. No UI
connected ⇒ the request is rejected immediately; no answer within
`PAIRING_DECISION_TIMEOUT` (30s) ⇒ rejected; only one request is prompted
at a time (others rejected while one is pending). This replaces the
earlier stage-based `is_pairing_window_open` stand-in.
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flow-daemon` (whole crate — lib + all integration targets now build).
Expected: PASS except `daemon/tests/pairing_over_channel.rs`, handled in Task 10. If that target blocks the run, temporarily `cargo test -p flow-daemon --lib` + `--test ipc_protocol` and note it.

- [ ] **Step 5: Commit**

```bash
git add daemon/src/ipc/server.rs daemon/src/main.rs daemon/README.md
git commit -m "feat(daemon): emit incoming_pairing_request_changed; pass peer addr"
```

---

### Task 10: Rewrite the end-to-end pairing integration test

**Files:**
- Modify: `daemon/tests/pairing_over_channel.rs`

**Interfaces:**
- Consumes: Task 7 (`watch_incoming_request`, `respond_to_pairing_request`, `accept_pairing_request(channel, peer_addr)`), Task 5 (`register_ipc_client`).

- [ ] **Step 1: Update the test to the prompt flow**

In `two_daemons_complete_a_real_pairing_handshake_over_tcp`:

1. Remove the `service_b.start_pairing()` "second press" block (lines ~72-75) and its comment.
2. Before spawning the responder task, mark B as having a connected UI:
   ```rust
   let _b_ui = service_b.register_ipc_client();
   ```
3. The responder task now needs the peer address; `accept_pairing_request` takes it as a second arg:
   ```rust
   let (stream, peer) = listener.accept().await.expect("accept");
   let channel = TcpChannel::accept(stream).await.expect("accept ws");
   service_b
       .accept_pairing_request(Box::new(channel), Some(peer))
       .await
       .expect("responder side of the handshake");
   ```
4. Add a concurrent task that acts as B's UI: watch and accept.
   ```rust
   let ui_b = {
       let service_b = service_b.clone();
       tokio::spawn(async move {
           let mut reqs = service_b.watch_incoming_request();
           loop {
               if let Some(req) = reqs.borrow_and_update().clone() {
                   service_b
                       .respond_to_pairing_request(&req.request_id, PairingDecision::Accept)
                       .await
                       .expect("ui accepts");
                   return;
               }
               reqs.changed().await.expect("incoming request stream");
           }
       })
   };
   ```
   `use flow_core::pairing::PairingDecision;` at the top.
5. After the initiator reaches `Paired`, `ui_b.await.expect("ui task");` alongside the existing `responder.await`.
6. Keep the existing assertion that B persisted the initiator with a real `public_key` (not `None`).

- [ ] **Step 2: Add a reject variant**

New `#[tokio::test] async fn a_rejected_incoming_request_fails_the_initiator()`: same wiring, but the UI task calls `respond_to_pairing_request(id, PairingDecision::Reject)`. Assert: `service_a`'s session ends at `PairingStage::Failed`; `storage_b`'s device repo has no record for the initiator's key.

- [ ] **Step 3: Run tests**

Run: `cargo test -p flow-daemon --test pairing_over_channel`
Expected: PASS (both tests).

- [ ] **Step 4: Full daemon suite + clippy**

Run: `cargo test -p flow-daemon` then `cargo clippy -p flow-daemon --all-targets -- -D warnings`
Expected: PASS / no warnings.

- [ ] **Step 5: Commit**

```bash
git add daemon/tests/pairing_over_channel.rs
git commit -m "test(daemon): pairing_over_channel drives respond_to_pairing_request"
```

---

## Phase C — Flutter Track 1

### Task 11: `IncomingPairingRequest` + `PairingDecision` domain types

**Files:**
- Modify: `flutter/lib/domain/pairing.dart`
- Test: `flutter/test/domain/pairing_test.dart` (create if absent; otherwise add to the existing pairing domain test)

**Interfaces:**
- Produces:
  - `enum PairingDecision { accept, reject }` with `String get wireName` (`accept`/`reject`).
  - `class IncomingPairingRequest { final String requestId; final String deviceName; final HostOs deviceOs; final String fingerprint; final String address; }` with `==`/`hashCode` and `static IncomingPairingRequest? incomingPairingRequestFromJson(Object? json)` (returns `null` when `json` is `null`).

- [ ] **Step 1: Write the failing test**

`flutter/test/domain/pairing_test.dart`:

```dart
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/pairing.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('incomingPairingRequestFromJson parses a full payload', () {
    final req = incomingPairingRequestFromJson({
      'request_id': 'ipr-1',
      'device_name': 'Windows Box',
      'device_os': 'windows',
      'fingerprint': '3f2a 91c4 8d10 6b57',
      'address': '192.168.0.103',
    });
    expect(req, isNotNull);
    expect(req!.requestId, 'ipr-1');
    expect(req.deviceOs, HostOs.windows);
    expect(req.fingerprint, '3f2a 91c4 8d10 6b57');
  });

  test('incomingPairingRequestFromJson returns null for null', () {
    expect(incomingPairingRequestFromJson(null), isNull);
  });

  test('PairingDecision.wireName', () {
    expect(PairingDecision.accept.wireName, 'accept');
    expect(PairingDecision.reject.wireName, 'reject');
  });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run (from `flutter/`): `flutter test test/domain/pairing_test.dart`
Expected: FAIL — `incomingPairingRequestFromJson` undefined.

- [ ] **Step 3: Write minimal implementation**

Append to `flutter/lib/domain/pairing.dart` (it already imports `device.dart` for `HostOs`; reuse the `hostOsFromJson` helper if one exists in the codebase — search; otherwise inline the switch):

```dart
/// The local user's answer to an incoming pairing request.
enum PairingDecision {
  accept,
  reject;

  String get wireName => switch (this) {
    PairingDecision.accept => 'accept',
    PairingDecision.reject => 'reject',
  };
}

/// An incoming pairing request awaiting this user's Accept/Reject.
/// Mirrors `docs/contracts/data-model.md`'s `IncomingPairingRequest`.
class IncomingPairingRequest {
  const IncomingPairingRequest({
    required this.requestId,
    required this.deviceName,
    required this.deviceOs,
    required this.fingerprint,
    required this.address,
  });

  final String requestId;
  final String deviceName;
  final HostOs deviceOs;
  final String fingerprint;
  final String address;

  @override
  bool operator ==(Object other) =>
      other is IncomingPairingRequest &&
      other.requestId == requestId &&
      other.deviceName == deviceName &&
      other.deviceOs == deviceOs &&
      other.fingerprint == fingerprint &&
      other.address == address;

  @override
  int get hashCode =>
      Object.hash(requestId, deviceName, deviceOs, fingerprint, address);
}

/// `null` in ⇒ `null` out (the stream carries `null` when nothing is pending).
IncomingPairingRequest? incomingPairingRequestFromJson(Object? json) {
  if (json == null) return null;
  final map = json as Map<String, dynamic>;
  return IncomingPairingRequest(
    requestId: map['request_id'] as String,
    deviceName: map['device_name'] as String,
    deviceOs: switch (map['device_os'] as String) {
      'macos' => HostOs.macos,
      'windows' => HostOs.windows,
      _ => HostOs.linux,
    },
    fingerprint: map['fingerprint'] as String,
    address: map['address'] as String? ?? '',
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `flutter test test/domain/pairing_test.dart`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add flutter/lib/domain/pairing.dart flutter/test/domain/pairing_test.dart
git commit -m "feat(flutter): IncomingPairingRequest + PairingDecision domain types"
```

---

### Task 12: Interface members + `MockDaemonRepository` implementation

**Files:**
- Modify: `flutter/lib/domain/daemon_repository.dart`
- Modify: `flutter/lib/data/mock_daemon_repository.dart`
- Test: `flutter/test/data/mock_daemon_repository_test.dart`

**Interfaces:**
- Consumes: Task 11 types.
- Produces:
  - `DaemonRepository.watchIncomingPairingRequest() -> Stream<IncomingPairingRequest?>`
  - `DaemonRepository.respondToPairingRequest(String requestId, PairingDecision decision) -> Future<void>`
  - `MockDaemonRepository.simulateIncomingPairingRequest({required String deviceName, required HostOs deviceOs, String fingerprint, String address})` — **not** on the interface; test/harness hook.

- [ ] **Step 1: Write the failing test**

Add to `flutter/test/data/mock_daemon_repository_test.dart`:

```dart
test('simulate + accept adds a device and clears the stream', () async {
  final repo = MockDaemonRepository();
  addTearDown(repo.dispose);

  final seen = <IncomingPairingRequest?>[];
  final sub = repo.watchIncomingPairingRequest().listen(seen.add);
  await Future<void>.delayed(Duration.zero);
  expect(seen.last, isNull);

  repo.simulateIncomingPairingRequest(
    deviceName: 'Studio Linux',
    deviceOs: HostOs.linux,
  );
  await Future<void>.delayed(Duration.zero);
  final pending = seen.last;
  expect(pending, isNotNull);

  await repo.respondToPairingRequest(pending!.requestId, PairingDecision.accept);
  await Future<void>.delayed(Duration.zero);
  expect(seen.last, isNull);

  final devices = await repo.watchDevices().first;
  expect(devices.any((d) => d.name == 'Studio Linux'), isTrue);

  await sub.cancel();
});

test('respondToPairingRequest with unknown id throws pairing_request_not_found',
    () async {
  final repo = MockDaemonRepository();
  addTearDown(repo.dispose);
  expect(
    () => repo.respondToPairingRequest('ipr-nope', PairingDecision.reject),
    throwsA(isA<DaemonCommandException>()
        .having((e) => e.code, 'code', 'pairing_request_not_found')),
  );
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `flutter test test/data/mock_daemon_repository_test.dart`
Expected: FAIL — method not defined.

- [ ] **Step 3: Write minimal implementation**

`flutter/lib/domain/daemon_repository.dart` — add to the abstract class:

```dart
  Stream<IncomingPairingRequest?> watchIncomingPairingRequest();
  Future<void> respondToPairingRequest(
    String requestId,
    PairingDecision decision,
  );
```

(ensure `import 'pairing.dart';` covers the new types — it already imports it for `PairingSession`.)

`flutter/lib/data/mock_daemon_repository.dart`:

- Field + init:
  ```dart
  late final ReplayChannel<IncomingPairingRequest?> _incomingRequest;
  int _nextRequestId = 0;
  ```
  In the constructor: `_incomingRequest = ReplayChannel<IncomingPairingRequest?>(null);`
- Interface methods:
  ```dart
  @override
  Stream<IncomingPairingRequest?> watchIncomingPairingRequest() =>
      _incomingRequest.watch();

  @override
  Future<void> respondToPairingRequest(
    String requestId,
    PairingDecision decision,
  ) async {
    final pending = _incomingRequest.value;
    if (pending == null || pending.requestId != requestId) {
      throw const DaemonCommandException(
        'pairing_request_not_found',
        'no pairing request is awaiting a decision for that id',
      );
    }
    _incomingRequest.emit(null);
    if (decision == PairingDecision.accept) {
      final now = DateTime.now();
      _devices.emit([
        ..._devices.value,
        Device(
          id: 'paired-${now.microsecondsSinceEpoch}',
          name: pending.deviceName,
          os: pending.deviceOs,
          state: DeviceState.inactive,
          lastSeen: now,
        ),
      ]);
    }
  }
  ```
  (match the real `Device` constructor param names in this codebase — check `_seedDevices()` in this same file.)
- Harness hook:
  ```dart
  /// Test/dev-harness only — the mock has no network to receive a real
  /// request from. Pushes one onto `watchIncomingPairingRequest()`.
  void simulateIncomingPairingRequest({
    required String deviceName,
    required HostOs deviceOs,
    String fingerprint = 'ab12 cd34 ef56 7890',
    String address = '192.168.1.42',
  }) {
    _incomingRequest.emit(IncomingPairingRequest(
      requestId: 'ipr-${_nextRequestId++}',
      deviceName: deviceName,
      deviceOs: deviceOs,
      fingerprint: fingerprint,
      address: address,
    ));
  }
  ```
- In `dispose()`: add `_incomingRequest.close();`.
- Add imports if needed (`IncomingPairingRequest`, `PairingDecision` come from `pairing.dart`, already imported).

- [ ] **Step 4: Run tests to verify they pass**

Run: `flutter test test/data/mock_daemon_repository_test.dart`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add flutter/lib/domain/daemon_repository.dart flutter/lib/data/mock_daemon_repository.dart flutter/test/data/mock_daemon_repository_test.dart
git commit -m "feat(flutter): incoming-pairing-request on DaemonRepository + mock"
```

---

### Task 13: `IpcDaemonRepository` wiring

**Files:**
- Modify: `flutter/lib/data/ipc_daemon_repository.dart`
- Test: `flutter/test/data/ipc_daemon_repository_test.dart`

**Interfaces:**
- Consumes: Task 11 (`incomingPairingRequestFromJson`, `PairingDecision`), Task 12 (interface members).
- Produces: routes event `incoming_pairing_request_changed`; `respondToPairingRequest` sends `{ id, command: "respond_to_pairing_request", payload: { request_id, decision } }`.

- [ ] **Step 1: Write the failing test**

Add to `flutter/test/data/ipc_daemon_repository_test.dart` (match the in-memory `StreamChannelController` pattern already in that file):

```dart
test('routes incoming_pairing_request_changed onto the stream', () async {
  final controller = StreamChannelController<dynamic>(allowForeignErrors: false);
  final repo = IpcDaemonRepository.withChannel(controller.foreign);
  addTearDown(repo.dispose);

  final seen = <IncomingPairingRequest?>[];
  repo.watchIncomingPairingRequest().listen(seen.add);

  controller.local.sink.add(jsonEncode({
    'event': 'incoming_pairing_request_changed',
    'payload': {
      'request_id': 'ipr-7',
      'device_name': 'Windows Box',
      'device_os': 'windows',
      'fingerprint': '3f2a 91c4 8d10 6b57',
      'address': '10.0.0.5',
    },
  }));
  await Future<void>.delayed(Duration.zero);
  expect(seen.last?.requestId, 'ipr-7');

  controller.local.sink.add(jsonEncode({
    'event': 'incoming_pairing_request_changed',
    'payload': null,
  }));
  await Future<void>.delayed(Duration.zero);
  expect(seen.last, isNull);
});

test('respondToPairingRequest sends the correct frame', () async {
  final controller = StreamChannelController<dynamic>(allowForeignErrors: false);
  final repo = IpcDaemonRepository.withChannel(controller.foreign);
  addTearDown(repo.dispose);

  final sent = <Map<String, dynamic>>[];
  controller.local.stream.listen((data) =>
      sent.add(jsonDecode(data as String) as Map<String, dynamic>));

  unawaited(repo.respondToPairingRequest('ipr-7', PairingDecision.accept));
  await Future<void>.delayed(Duration.zero);

  expect(sent.single['command'], 'respond_to_pairing_request');
  expect(sent.single['payload'], {'request_id': 'ipr-7', 'decision': 'accept'});
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `flutter test test/data/ipc_daemon_repository_test.dart`
Expected: FAIL — method/stream not present.

- [ ] **Step 3: Write minimal implementation**

`flutter/lib/data/ipc_daemon_repository.dart`:

- Field with the others:
  ```dart
  final _incomingRequest = ReplayChannel<IncomingPairingRequest?>();
  ```
- In `_handleEvent`'s `switch`, add:
  ```dart
      case 'incoming_pairing_request_changed':
        _incomingRequest.emit(incomingPairingRequestFromJson(payload));
  ```
- Interface methods (near `startPairing`/`cancelPairing`):
  ```dart
  @override
  Stream<IncomingPairingRequest?> watchIncomingPairingRequest() =>
      _incomingRequest.watch();

  @override
  Future<void> respondToPairingRequest(
    String requestId,
    PairingDecision decision,
  ) => _sendCommand('respond_to_pairing_request', {
        'request_id': requestId,
        'decision': decision.wireName,
      });
  ```
- `_failChannelsAwaitingFirstValue`: add `_incomingRequest` to the list.
- `dispose()`: add `_incomingRequest.close();`.
- Note: `_incomingRequest` starts with no value, so `_failChannelsAwaitingFirstValue` will surface a transport error on it before its first frame — consistent with the other channels. The daemon sends `incoming_pairing_request_changed: null` as the 6th initial event, which becomes its first value on a healthy connection.

- [ ] **Step 4: Run tests to verify they pass**

Run: `flutter test test/data/ipc_daemon_repository_test.dart`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add flutter/lib/data/ipc_daemon_repository.dart flutter/test/data/ipc_daemon_repository_test.dart
git commit -m "feat(flutter): route incoming_pairing_request_changed + respond command"
```

---

### Task 14: Provider + the modal dialog widget

**Files:**
- Modify: `flutter/lib/state/repository_providers.dart`
- Create: `flutter/lib/features/pairing/incoming_pairing_request_dialog.dart`
- Test: `flutter/test/features/pairing/incoming_pairing_request_dialog_test.dart`

**Interfaces:**
- Produces:
  - `incomingPairingRequestProvider = StreamProvider<IncomingPairingRequest?>`
  - `class IncomingPairingRequestDialog extends StatelessWidget` — constructed with `{ required IncomingPairingRequest request }`; `showIncomingPairingRequestDialog(BuildContext, IncomingPairingRequest) -> Future<PairingDecision?>` (returns `null` if dismissed).

- [ ] **Step 1: Write the failing test**

`flutter/test/features/pairing/incoming_pairing_request_dialog_test.dart`:

```dart
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/pairing.dart';
import 'package:flow_ui/features/pairing/incoming_pairing_request_dialog.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final request = const IncomingPairingRequest(
    requestId: 'ipr-1',
    deviceName: 'Windows Box',
    deviceOs: HostOs.windows,
    fingerprint: '3f2a 91c4 8d10 6b57',
    address: '192.168.0.103',
  );

  testWidgets('renders identity and returns Accept', (tester) async {
    late Future<PairingDecision?> result;
    await tester.pumpWidget(MaterialApp(
      home: Builder(
        builder: (context) => ElevatedButton(
          onPressed: () =>
              result = showIncomingPairingRequestDialog(context, request),
          child: const Text('go'),
        ),
      ),
    ));
    await tester.tap(find.text('go'));
    await tester.pumpAndSettle();

    expect(find.text('Windows Box'), findsOneWidget);
    expect(find.textContaining('3f2a 91c4 8d10 6b57'), findsOneWidget);
    expect(find.textContaining('192.168.0.103'), findsOneWidget);

    await tester.tap(find.text('Accept'));
    await tester.pumpAndSettle();
    expect(await result, PairingDecision.accept);
  });

  testWidgets('Reject returns reject', (tester) async {
    late Future<PairingDecision?> result;
    await tester.pumpWidget(MaterialApp(
      home: Builder(
        builder: (context) => ElevatedButton(
          onPressed: () =>
              result = showIncomingPairingRequestDialog(context, request),
          child: const Text('go'),
        ),
      ),
    ));
    await tester.tap(find.text('go'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Reject'));
    await tester.pumpAndSettle();
    expect(await result, PairingDecision.reject);
  });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `flutter test test/features/pairing/incoming_pairing_request_dialog_test.dart`
Expected: FAIL — file/symbol missing.

- [ ] **Step 3: Write minimal implementation**

Add to `flutter/lib/state/repository_providers.dart` (next to `pairingSessionProvider`):

```dart
final incomingPairingRequestProvider = StreamProvider<IncomingPairingRequest?>((
  ref,
) {
  return ref.watch(daemonRepositoryProvider).watchIncomingPairingRequest();
});
```

Create `flutter/lib/features/pairing/incoming_pairing_request_dialog.dart`:

```dart
import 'package:flutter/material.dart';

import '../../domain/device.dart';
import '../../domain/pairing.dart';

/// Modal shown when another device asks to pair with this one. The only
/// consent gate — there is no auto-accept. Returns the user's choice, or
/// `null` if the dialog was dismissed (caller treats that as reject).
Future<PairingDecision?> showIncomingPairingRequestDialog(
  BuildContext context,
  IncomingPairingRequest request,
) {
  return showDialog<PairingDecision>(
    context: context,
    barrierDismissible: true,
    builder: (_) => IncomingPairingRequestDialog(request: request),
  );
}

class IncomingPairingRequestDialog extends StatelessWidget {
  const IncomingPairingRequestDialog({super.key, required this.request});

  final IncomingPairingRequest request;

  String get _osLabel => switch (request.deviceOs) {
    HostOs.macos => 'macOS',
    HostOs.windows => 'Windows',
    HostOs.linux => 'Linux',
  };

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Pair this device?'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            request.deviceName,
            style: Theme.of(context).textTheme.titleMedium,
          ),
          const SizedBox(height: 4),
          Text(
            request.address.isEmpty
                ? _osLabel
                : '$_osLabel · ${request.address}',
            style: Theme.of(context).textTheme.bodySmall,
          ),
          const SizedBox(height: 16),
          const Text('Verification code'),
          const SizedBox(height: 4),
          SelectableText(
            request.fingerprint,
            style: const TextStyle(
              fontFamily: 'monospace',
              fontSize: 16,
              letterSpacing: 1.5,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            'Confirm this matches the code shown on the other device.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () =>
              Navigator.of(context).pop(PairingDecision.reject),
          child: const Text('Reject'),
        ),
        FilledButton(
          onPressed: () =>
              Navigator.of(context).pop(PairingDecision.accept),
          child: const Text('Accept'),
        ),
      ],
    );
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `flutter test test/features/pairing/incoming_pairing_request_dialog_test.dart`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add flutter/lib/state/repository_providers.dart flutter/lib/features/pairing/incoming_pairing_request_dialog.dart flutter/test/features/pairing/incoming_pairing_request_dialog_test.dart
git commit -m "feat(flutter): incoming pairing request dialog + provider"
```

---

### Task 15: Root listener that shows the dialog and answers the daemon

**Files:**
- Create: `flutter/lib/features/pairing/incoming_pairing_request_listener.dart`
- Modify: `flutter/lib/app.dart` (mount the listener; factor a public `showMainWindow()`)
- Modify: `flutter/lib/features/harness/dev_harness.dart` (mount the listener + a "Simulate incoming pairing request" button)
- Test: `flutter/test/features/pairing/incoming_pairing_request_listener_test.dart`

**Interfaces:**
- Consumes: Task 12 (`respondToPairingRequest`), Task 14 (`incomingPairingRequestProvider`, `showIncomingPairingRequestDialog`).
- Produces: `class IncomingPairingRequestListener extends ConsumerStatefulWidget` — wraps `child`; on a non-null request shows the dialog (once), on null pops it, routes the result to `respondToPairingRequest`, swallows `pairing_request_not_found`. Accepts an optional `void Function()? onShouldSurfaceWindow` (app passes `showMainWindow`; harness passes `null`).

- [ ] **Step 1: Write the failing test**

`flutter/test/features/pairing/incoming_pairing_request_listener_test.dart`:

```dart
import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/pairing.dart';
import 'package:flow_ui/features/pairing/incoming_pairing_request_listener.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('shows dialog on request, Accept calls repo, null pops it',
      (tester) async {
    final repo = MockDaemonRepository();
    addTearDown(repo.dispose);
    final container = ProviderContainer(
      overrides: [daemonRepositoryProvider.overrideWithValue(repo)],
    );
    addTearDown(container.dispose);

    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: const MaterialApp(
        home: IncomingPairingRequestListener(child: Scaffold(body: SizedBox())),
      ),
    ));
    await tester.pump();

    repo.simulateIncomingPairingRequest(
      deviceName: 'Windows Box',
      deviceOs: HostOs.windows,
    );
    await tester.pumpAndSettle();
    expect(find.text('Windows Box'), findsOneWidget);

    await tester.tap(find.text('Accept'));
    await tester.pumpAndSettle();

    // Mock cleared the stream and added the device.
    final devices = await repo.watchDevices().first;
    expect(devices.any((d) => d.name == 'Windows Box'), isTrue);
    expect(find.text('Windows Box'), findsNothing);
  });

  testWidgets('stream going null while open dismisses the dialog',
      (tester) async {
    final repo = MockDaemonRepository();
    addTearDown(repo.dispose);
    final container = ProviderContainer(
      overrides: [daemonRepositoryProvider.overrideWithValue(repo)],
    );
    addTearDown(container.dispose);

    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: const MaterialApp(
        home: IncomingPairingRequestListener(child: Scaffold(body: SizedBox())),
      ),
    ));
    await tester.pump();

    repo.simulateIncomingPairingRequest(
      deviceName: 'Studio Linux',
      deviceOs: HostOs.linux,
    );
    await tester.pumpAndSettle();
    expect(find.text('Studio Linux'), findsOneWidget);

    // Someone else answered / it timed out on the daemon.
    await repo.respondToPairingRequest(
      (await repo.watchIncomingPairingRequest().first)!.requestId,
      PairingDecision.reject,
    );
    await tester.pumpAndSettle();
    expect(find.text('Studio Linux'), findsNothing);
  });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `flutter test test/features/pairing/incoming_pairing_request_listener_test.dart`
Expected: FAIL — file missing.

- [ ] **Step 3: Write minimal implementation**

Create `flutter/lib/features/pairing/incoming_pairing_request_listener.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/daemon_command_exception.dart';
import '../../domain/pairing.dart';
import '../../state/repository_providers.dart';
import 'incoming_pairing_request_dialog.dart';

/// Mounted once, high in the tree. Turns the `incomingPairingRequestProvider`
/// stream into exactly one modal at a time: shows it when a request
/// arrives, pops it when the stream clears (daemon timeout, withdrawal,
/// or another surface answered), and routes the user's choice back to
/// the daemon.
class IncomingPairingRequestListener extends ConsumerStatefulWidget {
  const IncomingPairingRequestListener({
    super.key,
    required this.child,
    this.onShouldSurfaceWindow,
  });

  final Widget child;

  /// The shipped app passes a callback that raises/focuses the OS window;
  /// the dev harness passes `null`.
  final void Function()? onShouldSurfaceWindow;

  @override
  ConsumerState<IncomingPairingRequestListener> createState() =>
      _IncomingPairingRequestListenerState();
}

class _IncomingPairingRequestListenerState
    extends ConsumerState<IncomingPairingRequestListener> {
  /// The request id currently on screen, if a dialog is open.
  String? _shownRequestId;

  @override
  Widget build(BuildContext context) {
    ref.listen(incomingPairingRequestProvider, (previous, next) {
      final request = next.valueOrNull;
      if (request != null && _shownRequestId == null) {
        _show(request);
      } else if (request == null && _shownRequestId != null) {
        _dismiss();
      }
    });
    return widget.child;
  }

  Future<void> _show(IncomingPairingRequest request) async {
    _shownRequestId = request.requestId;
    widget.onShouldSurfaceWindow?.call();

    final decision =
        await showIncomingPairingRequestDialog(context, request) ??
        PairingDecision.reject;

    // Dialog closed by our own _dismiss() (stream already cleared): nothing to send.
    if (_shownRequestId != request.requestId) return;
    _shownRequestId = null;

    try {
      await ref
          .read(daemonRepositoryProvider)
          .respondToPairingRequest(request.requestId, decision);
    } on DaemonCommandException catch (e) {
      if (e.code != 'pairing_request_not_found') rethrow;
    }
  }

  void _dismiss() {
    _shownRequestId = null;
    final navigator = Navigator.of(context, rootNavigator: true);
    if (navigator.canPop()) navigator.pop();
  }
}
```

`flutter/lib/app.dart`:

- Factor the window-raise into a top-level function or a public static so the listener can call it:
  ```dart
  Future<void> showMainWindow() async {
    try {
      await windowManager.show();
      await windowManager.focus();
    } catch (_) {
      // No window plugin in this environment.
    }
  }
  ```
  and have `_showWindow()` delegate to it.
- Wrap `_RealApp`'s built `content` (inside the `Scaffold` body, around `Center(child: content)`):
  ```dart
  child: IncomingPairingRequestListener(
    onShouldSurfaceWindow: () => unawaited(showMainWindow()),
    child: Center(child: content),
  ),
  ```
- `import 'features/pairing/incoming_pairing_request_listener.dart';`

`flutter/lib/features/harness/dev_harness.dart`:

- Wrap the harness scaffold body in `IncomingPairingRequestListener(child: ...)` (no `onShouldSurfaceWindow`).
- Add a button near the existing tray/pairing controls:
  ```dart
  ElevatedButton(
    onPressed: () => (ref.read(daemonRepositoryProvider) as MockDaemonRepository)
        .simulateIncomingPairingRequest(
      deviceName: 'Office Mac Mini',
      deviceOs: HostOs.macos,
    ),
    child: const Text('Simulate incoming pairing request'),
  )
  ```
  (import `MockDaemonRepository` if not already; the harness always runs on the mock backend.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `flutter test test/features/pairing/`
Expected: PASS (dialog + listener).

- [ ] **Step 5: Commit**

```bash
git add flutter/lib/features/pairing/incoming_pairing_request_listener.dart flutter/lib/app.dart flutter/lib/features/harness/dev_harness.dart flutter/test/features/pairing/incoming_pairing_request_listener_test.dart
git commit -m "feat(flutter): root listener that prompts for incoming pairing requests"
```

---

### Task 16: End-to-end UI flow test

**Files:**
- Modify: `flutter/test/e2e/daemon_ui_flow_e2e_test.dart`

**Interfaces:**
- Consumes: Tasks 12, 14, 15.

- [ ] **Step 1: Add the failing test**

Add a group/test to `daemon_ui_flow_e2e_test.dart` following its existing `harness(...)` helper (which wires a `MockDaemonRepository` + real widgets). Mount `IncomingPairingRequestListener` around the widget under test (or add it to the shared harness), then:

```dart
testWidgets('incoming pairing request → Accept adds the device', (tester) async {
  final repo = MockDaemonRepository();
  addTearDown(repo.dispose);
  await tester.pumpWidget(harnessWith(repo, const SizedBox()));
  await tester.pump();

  repo.simulateIncomingPairingRequest(
    deviceName: 'Nearby Laptop',
    deviceOs: HostOs.windows,
  );
  await tester.pumpAndSettle();
  await tester.tap(find.text('Accept'));
  await tester.pumpAndSettle();

  final devices = await repo.watchDevices().first;
  expect(devices.any((d) => d.name == 'Nearby Laptop'), isTrue);
});
```

Adapt `harnessWith` to the file's actual helper name/shape; if the shared harness doesn't include the listener, wrap inline.

- [ ] **Step 2: Run test to verify it fails, then passes**

Run: `flutter test test/e2e/daemon_ui_flow_e2e_test.dart`
Expected: FAIL first (if the listener isn't in the harness), then PASS once wired.

- [ ] **Step 3: Full Flutter suite**

Run: `flutter analyze` then `flutter test`
Expected: no analyzer issues; all tests pass.

- [ ] **Step 4: Commit**

```bash
git add flutter/test/e2e/daemon_ui_flow_e2e_test.dart
git commit -m "test(flutter): e2e incoming pairing request accept flow"
```

---

## Phase D — Track 2: tray native menu

### Task 17: `buildTrayMenu()` pure function

**Files:**
- Create: `flutter/lib/features/tray/tray_menu.dart`
- Test: `flutter/test/features/tray/tray_menu_test.dart`

**Interfaces:**
- Consumes: `DaemonLinkState`, `Device`, `DeviceState`, `HostOs`.
- Produces:
  - `sealed class TrayAction` with `SwitchDevice(String deviceId)`, `PairNewDevice`, `OpenDashboard`, `OpenSettings`, `QuitApp` (use a plain `enum` + optional id, or a sealed class — sealed class shown below).
  - `class TrayMenuEntry { final String label; final bool enabled; final TrayAction? action; final bool isSeparator; }`
  - `List<TrayMenuEntry> buildTrayMenu({required DaemonLinkState link, required List<Device> devices, required String localDeviceId})`

- [ ] **Step 1: Write the failing test**

`flutter/test/features/tray/tray_menu_test.dart`:

```dart
import 'package:flow_ui/domain/daemon_link_state.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/features/tray/tray_menu.dart';
import 'package:flutter_test/flutter_test.dart';

Device _dev(String id, String name, DeviceState state) => Device(
      id: id,
      name: name,
      os: HostOs.macos,
      state: state,
      lastSeen: DateTime(2026),
    );

void main() {
  test('connected: status, active, switch target, disconnected, menu rows', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.connected,
      localDeviceId: 'd1',
      devices: [
        _dev('d1', 'MacBook', DeviceState.active),
        _dev('d2', 'Work Laptop', DeviceState.inactive),
        _dev('d3', 'Desktop', DeviceState.disconnected),
      ],
    );
    final labels = entries.where((e) => !e.isSeparator).map((e) => e.label).toList();
    expect(labels, [
      'Flow — Connected',
      'Using: MacBook (macOS)',
      'Switch to Work Laptop',
      'Desktop — disconnected',
      'Pair New Device…',
      'Dashboard',
      'Settings',
      'Quit Flow',
    ]);

    final switchRow = entries.firstWhere((e) => e.label == 'Switch to Work Laptop');
    expect(switchRow.enabled, isTrue);
    expect(switchRow.action, const TrayAction.switchDevice('d2'));

    final disc = entries.firstWhere((e) => e.label == 'Desktop — disconnected');
    expect(disc.enabled, isFalse);

    expect(
      entries.firstWhere((e) => e.label == 'Flow — Connected').enabled,
      isFalse,
    );
  });

  test('no active device: no "Using:" row', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.connecting,
      localDeviceId: 'd1',
      devices: [_dev('d2', 'Work Laptop', DeviceState.inactive)],
    );
    expect(entries.any((e) => e.label.startsWith('Using:')), isFalse);
    expect(entries.first.label, 'Flow — Connecting…');
  });

  test('permissionRequired status label', () {
    final entries = buildTrayMenu(
      link: DaemonLinkState.permissionRequired,
      localDeviceId: 'd1',
      devices: const [],
    );
    expect(entries.first.label, 'Flow — Needs permission');
  });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `flutter test test/features/tray/tray_menu_test.dart`
Expected: FAIL — file missing.

- [ ] **Step 3: Write minimal implementation**

Create `flutter/lib/features/tray/tray_menu.dart`:

```dart
import '../../domain/daemon_link_state.dart';
import '../../domain/device.dart';

/// What a tray menu row does when clicked. `null` action ⇒ a disabled
/// informational row.
sealed class TrayAction {
  const TrayAction();
  const factory TrayAction.switchDevice(String deviceId) = _SwitchDevice;
  static const pairNewDevice = _PairNewDevice();
  static const openDashboard = _OpenDashboard();
  static const openSettings = _OpenSettings();
  static const quitApp = _QuitApp();
}

class _SwitchDevice extends TrayAction {
  const _SwitchDevice(this.deviceId);
  final String deviceId;
  @override
  bool operator ==(Object other) =>
      other is _SwitchDevice && other.deviceId == deviceId;
  @override
  int get hashCode => deviceId.hashCode;
}

class _PairNewDevice extends TrayAction {
  const _PairNewDevice();
}

class _OpenDashboard extends TrayAction {
  const _OpenDashboard();
}

class _OpenSettings extends TrayAction {
  const _OpenSettings();
}

class _QuitApp extends TrayAction {
  const _QuitApp();
}

class TrayMenuEntry {
  const TrayMenuEntry({
    required this.label,
    this.enabled = true,
    this.action,
    this.isSeparator = false,
  });

  const TrayMenuEntry.separator()
      : label = '',
        enabled = false,
        action = null,
        isSeparator = true;

  final String label;
  final bool enabled;
  final TrayAction? action;
  final bool isSeparator;
}

String _linkLabel(DaemonLinkState link) => switch (link) {
  DaemonLinkState.connected => 'Connected',
  DaemonLinkState.connecting => 'Connecting…',
  DaemonLinkState.reconnecting => 'Reconnecting…',
  DaemonLinkState.disconnected => 'Disconnected',
  DaemonLinkState.error => 'Connection lost',
  DaemonLinkState.permissionRequired => 'Needs permission',
};

String _osLabel(HostOs os) => switch (os) {
  HostOs.macos => 'macOS',
  HostOs.windows => 'Windows',
  HostOs.linux => 'Linux',
};

/// The tray/menu-bar menu, derived from live daemon state. Mirrors the
/// information architecture of `design/claude-design-export/TrayPopover.dc.html`
/// as far as a native menu allows (no inline pairing progress).
List<TrayMenuEntry> buildTrayMenu({
  required DaemonLinkState link,
  required List<Device> devices,
  required String localDeviceId,
}) {
  final entries = <TrayMenuEntry>[
    TrayMenuEntry(label: 'Flow — ${_linkLabel(link)}', enabled: false),
    const TrayMenuEntry.separator(),
  ];

  final active = devices
      .where((d) => d.id != localDeviceId && d.state == DeviceState.active)
      .firstOrNull;
  // "This device" is the local record; show it as the active row when
  // nothing remote is active (matches the popover's "Using" card).
  final local = devices.where((d) => d.id == localDeviceId).firstOrNull;
  final usingDevice = active ?? local;
  if (usingDevice != null) {
    entries.add(TrayMenuEntry(
      label: 'Using: ${usingDevice.name} (${_osLabel(usingDevice.os)})',
      enabled: false,
    ));
  }

  for (final d in devices.where((d) => d.id != localDeviceId)) {
    if (d.state == DeviceState.inactive || d.state == DeviceState.connected) {
      entries.add(TrayMenuEntry(
        label: 'Switch to ${d.name}',
        action: TrayAction.switchDevice(d.id),
      ));
    } else if (d.state == DeviceState.disconnected ||
        d.state == DeviceState.error) {
      entries.add(TrayMenuEntry(
        label: '${d.name} — disconnected',
        enabled: false,
      ));
    }
  }

  entries.addAll(const [
    TrayMenuEntry.separator(),
    TrayMenuEntry(label: 'Pair New Device…', action: TrayAction.pairNewDevice),
    TrayMenuEntry.separator(),
    TrayMenuEntry(label: 'Dashboard', action: TrayAction.openDashboard),
    TrayMenuEntry(label: 'Settings', action: TrayAction.openSettings),
    TrayMenuEntry(label: 'Quit Flow', action: TrayAction.quitApp),
  ]);

  return entries;
}
```

> If `firstOrNull` isn't imported in this codebase's style, use
> `collection`'s extension (already a transitive dep via flutter) or a
> local `.cast<Device?>().firstWhere((_) => true, orElse: () => null)`
> — check how other files here do it (`mock_daemon_repository.dart` has
> an `extension<T> on Iterable<T>` at the bottom).

- [ ] **Step 4: Run tests to verify they pass**

Run: `flutter test test/features/tray/tray_menu_test.dart`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add flutter/lib/features/tray/tray_menu.dart flutter/test/features/tray/tray_menu_test.dart
git commit -m "feat(flutter): buildTrayMenu — native tray menu from daemon state"
```

---

### Task 18: Wire the native menu into `_RealApp`

**Files:**
- Modify: `flutter/lib/app.dart`
- Test: `flutter/test/features/tray/tray_menu_dispatch_test.dart` (create) — tests the action→effect mapping in isolation; the `tray_manager` calls themselves aren't unit-testable.

**Interfaces:**
- Consumes: Task 17 (`buildTrayMenu`, `TrayMenuEntry`, `TrayAction`), existing `AppWindowShell(initialSection:)`, `AppSection`.
- Produces: `_RealAppState` builds/rebuilds the tray menu from `devicesProvider` + `linkStateProvider`; left- and right-click both pop it; `TrayAction` dispatch runs the effects below.

- [ ] **Step 1: Write the failing test**

`flutter/test/features/tray/tray_menu_dispatch_test.dart` — extract the dispatch into a pure, testable function and test that:

```dart
import 'package:flow_ui/app.dart' show TrayActionEffect, resolveTrayAction;
import 'package:flow_ui/features/app_window/app_window_shell.dart';
import 'package:flow_ui/features/tray/tray_menu.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('switchDevice → switch effect with id', () {
    final e = resolveTrayAction(const TrayAction.switchDevice('d2'));
    expect(e, isA<SwitchDeviceEffect>());
    expect((e as SwitchDeviceEffect).deviceId, 'd2');
  });

  test('openSettings → show window on General', () {
    final e = resolveTrayAction(TrayAction.openSettings);
    expect(e, isA<ShowWindowEffect>());
    expect((e as ShowWindowEffect).section, AppSection.general);
  });

  test('pairNewDevice → start pairing then show dashboard', () {
    final e = resolveTrayAction(TrayAction.pairNewDevice);
    expect(e, isA<StartPairingThenShowEffect>());
    expect((e as StartPairingThenShowEffect).section, AppSection.dashboard);
  });

  test('quit → quit effect', () {
    expect(resolveTrayAction(TrayAction.quitApp), isA<QuitEffect>());
  });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `flutter test test/features/tray/tray_menu_dispatch_test.dart`
Expected: FAIL — symbols missing.

- [ ] **Step 3: Write minimal implementation**

In `flutter/lib/app.dart`:

- Add the effect model + resolver (top-level, exported):
  ```dart
  sealed class TrayActionEffect {
    const TrayActionEffect();
  }

  class SwitchDeviceEffect extends TrayActionEffect {
    const SwitchDeviceEffect(this.deviceId);
    final String deviceId;
  }

  class ShowWindowEffect extends TrayActionEffect {
    const ShowWindowEffect(this.section);
    final AppSection section;
  }

  class StartPairingThenShowEffect extends TrayActionEffect {
    const StartPairingThenShowEffect(this.section);
    final AppSection section;
  }

  class QuitEffect extends TrayActionEffect {
    const QuitEffect();
  }

  TrayActionEffect resolveTrayAction(TrayAction action) => switch (action) {
    _SwitchDevice() => SwitchDeviceEffect((action as dynamic).deviceId as String),
    _ when action == TrayAction.pairNewDevice =>
      const StartPairingThenShowEffect(AppSection.dashboard),
    _ when action == TrayAction.openDashboard =>
      const ShowWindowEffect(AppSection.dashboard),
    _ when action == TrayAction.openSettings =>
      const ShowWindowEffect(AppSection.general),
    _ when action == TrayAction.quitApp => const QuitEffect(),
    _ => const ShowWindowEffect(AppSection.dashboard),
  };
  ```
  > `TrayAction` in Task 17 hides its concrete subclasses. Either (a)
  > make `_SwitchDevice` (and the singletons) library-public in
  > `tray_menu.dart` so this `switch` can pattern-match them cleanly, or
  > (b) add a `String? get switchDeviceId` and `TrayActionKind get kind`
  > to `TrayAction` and match on that. Pick (b) if you don't want to
  > widen `tray_menu.dart`'s surface — adjust Task 17's file accordingly
  > and re-run its test. Keep the resolver total.

- Add to `_RealAppState`:
  ```dart
  AppSection _pendingSection = AppSection.dashboard;
  ProviderSubscription<AsyncValue<List<Device>>>? _devicesSub;
  ProviderSubscription<AsyncValue<DaemonLinkState>>? _linkSub;
  ```
  In `build`, after `_setUpTray()` is (or would be) called, keep the menu fresh:
  ```dart
  _devicesSub ??= ref.listenManual(devicesProvider, (_, __) => _rebuildTrayMenu());
  _linkSub ??= ref.listenManual(linkStateProvider, (_, __) => _rebuildTrayMenu());
  ```
  Dispose them in `dispose()`.

- Replace the static menu in `_setUpTray` with `await _rebuildTrayMenu();` and implement:
  ```dart
  Future<void> _rebuildTrayMenu() async {
    if (!_trayReady) return;
    final link = ref.read(linkStateProvider).valueOrNull ??
        DaemonLinkState.connecting;
    final devices = ref.read(devicesProvider).valueOrNull ?? const <Device>[];
    final entries = buildTrayMenu(
      link: link,
      devices: devices,
      localDeviceId: 'd1',
    );
    try {
      await trayManager.setContextMenu(Menu(
        items: [
          for (final e in entries)
            if (e.isSeparator)
              MenuItem.separator()
            else
              MenuItem(
                key: e.label,
                label: e.label,
                disabled: !e.enabled,
                onClick: e.action == null
                    ? null
                    : (_) => unawaited(_runTrayAction(e.action!)),
              ),
        ],
      ));
    } catch (_) {
      // No tray plugin in this environment.
    }
  }

  Future<void> _runTrayAction(TrayAction action) async {
    final effect = resolveTrayAction(action);
    final repo = ref.read(daemonRepositoryProvider);
    switch (effect) {
      case SwitchDeviceEffect(:final deviceId):
        try {
          await repo.switchActiveDevice(deviceId);
        } on DaemonCommandException catch (e) {
          ref.read(toastProvider.notifier).show(e.message);
        }
      case ShowWindowEffect(:final section):
        setState(() => _pendingSection = section);
        await showMainWindow();
      case StartPairingThenShowEffect(:final section):
        setState(() => _pendingSection = section);
        try {
          await repo.startPairing();
        } on DaemonCommandException {
          // already pairing — fine
        }
        await showMainWindow();
      case QuitEffect():
        await _quit();
    }
  }
  ```

- Change `onTrayIconMouseDown` to open the menu instead of toggling:
  ```dart
  @override
  void onTrayIconMouseDown() {
    if (!_trayReady) return;
    unawaited(_openTrayMenu());
  }

  @override
  void onTrayIconRightMouseDown() {
    if (!_trayReady) return;
    unawaited(_openTrayMenu());
  }

  Future<void> _openTrayMenu() async {
    await _rebuildTrayMenu();
    try {
      await trayManager.popUpContextMenu();
    } catch (_) {}
  }
  ```
  Delete `_toggleWindow` (now unused) — or keep it only if something else calls it (grep first).

- Pass the pending section to the shell:
  ```dart
  data: (complete) => complete
      ? AppWindowShell(
          key: ValueKey(_pendingSection),
          platform: platform,
          standalone: true,
          initialSection: _pendingSection,
        )
      : onboarding(),
  ```

- Ensure imports: `features/tray/tray_menu.dart`, `features/app_window/app_window_shell.dart` (for `AppSection`), `domain/daemon_command_exception.dart`, `state/repository_providers.dart`, `state/ui_providers.dart` (for `toastProvider`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `flutter test test/features/tray/` then `flutter analyze`
Expected: PASS; no analyzer issues. (`tray_manager` calls are guarded by `try/catch` and `_trayReady`, so widget tests that mount `_RealApp` — if any — still pass.)

- [ ] **Step 5: Manual verification note**

`tray_manager` has no test double. Record in the PR description that left-click opening the menu (not the window) was verified by `flutter run` on the dev machine, or defer to reviewer’s manual check.

- [ ] **Step 6: Commit**

```bash
git add flutter/lib/app.dart flutter/test/features/tray/tray_menu_dispatch_test.dart
git commit -m "feat(flutter): tray icon opens a native menu from daemon state"
```

---

### Task 19: Full-tree verification

**Files:** none (verification only)

- [ ] **Step 1: Rust**

Run:
```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```
Expected: all pass. Fix any fallout in the task that caused it and amend that commit or add a `fix:` commit.

- [ ] **Step 2: Flutter**

Run (from `flutter/`):
```
flutter analyze
flutter test
```
Expected: no issues; all tests green.

- [ ] **Step 3: Spec cross-check**

Re-open `docs/superpowers/specs/2026-08-28-incoming-pairing-prompt-and-tray-menu-design.md` and confirm each of §1.1–§1.5 and §2.1–§2.5 maps to a merged task. Note any deferred item explicitly in the PR description (SAS, "block this device", the glass popover panel).

- [ ] **Step 4: Commit / open PR**

```bash
git push -u origin feat/pairing-prompt-and-tray-menu
gh pr create --fill --base main
```

PR body: link issue #18, summarize both tracks, list the deferred follow-ups, and note the `tray_manager` manual-verification gap.

---

## Self-Review

**1. Spec coverage**

| Spec section | Task(s) |
|---|---|
| 1.1 Contract: `watchIncomingPairingRequest`, `respond_to_pairing_request`, 6 events, error code, `IncomingPairingRequest` in data-model, CHANGELOG | 3 (+ struct in 1, error in 2) |
| 1.2 `flow_core`: `IncomingPairingRequest`, `key_fingerprint` | 1 (struct); 6 (fingerprint — **moved to `flow_daemon`**, see Global Constraints) |
| 1.3 Daemon: handshake split | 4 |
| 1.3 Daemon: `incoming_request` state, `PAIRING_DECISION_TIMEOUT`, delete `is_pairing_window_open`, rewrite `accept_pairing_over`, `respond_to_pairing_request`, `connected_clients` | 5 (counter), 7 (rest) |
| 1.3 Daemon: `error.rs` code | 2 |
| 1.3 Daemon: `dispatch.rs` | 8 |
| 1.3 Daemon: `server.rs` 6th event + counter guard | 5, 9 |
| 1.3 Daemon: `main.rs` peer_addr + counter Arc | 9 (peer_addr); 5 (counter guard lives in server, Arc created in service ctor — no main.rs change needed, see Task 5) |
| 1.3 Daemon: README | 9 |
| 1.4 Flutter: models, interface, ipc repo, mock, provider, dialog, listener, harness | 11, 12, 13, 14, 15 |
| 1.5 Tests (daemon + flutter) | 7, 8, 9, 10 (daemon); 12, 13, 14, 15, 16 (flutter) |
| 2.1 Tray behavior: menu from live state, rebuild on change, both clicks open menu | 18 |
| 2.2 Menu contents | 17 |
| 2.3 Window deep-linking via `initialSection` + `ValueKey` | 18 |
| 2.4 `_setUpTray` changes | 18 |
| 2.5 Tray tests: pure `buildTrayMenu`, dispatch test | 17, 18 |
| Out of scope (SAS, block-device, popover panel) | not implemented; called out in Task 19 PR body |

No gaps.

**2. Placeholder scan** — code steps carry real code. Two spots delegate a lookup to the implementer with an explicit search target, not a vague "handle it": Task 7's in-memory `Channel` test-double path (repo-specific helper name) and Task 16's e2e harness helper name. Both name exactly what to find and why. Task 18 offers an A/B for `TrayAction` visibility with a concrete recommendation. Acceptable — these are genuine "match the existing pattern" points, each bounded.

**3. Type consistency**
- `IncomingPairingRequest` fields: `request_id/device_name/device_os/fingerprint/address` (Rust, Task 1) ↔ `requestId/deviceName/deviceOs/fingerprint/address` (Dart, Task 11) ↔ contract doc (Task 3). Consistent.
- Event name `incoming_pairing_request_changed`: Tasks 3, 9, 13. Consistent.
- Command `respond_to_pairing_request`: Tasks 3, 8, 13. Consistent.
- Error `pairing_request_not_found`: Tasks 2, 3, 8, 12, 15. Consistent.
- `PairingDecision.wireName` → `"accept"`/`"reject"` (Task 11) matches Rust serde `snake_case` (Task 8 payload) and dispatch. Consistent.
- `accept_pairing_over(channel, peer_public_key, peer_addr)` — defined Task 7, called Task 9. `accept_incoming_peer_channel(channel, peer_addr)` / `accept_pairing_request(channel, peer_addr)` — Task 7 defines, Tasks 9 & 10 call. Consistent.
- `register_ipc_client()`/`IpcClientGuard`/`connected_client_count()` — Task 5 defines, Tasks 7 (count check) & 10 (test guard) consume. Consistent.
- `buildTrayMenu({link, devices, localDeviceId})` — Task 17 defines, Task 18 calls with the same names. Consistent.
- `resolveTrayAction` / `TrayActionEffect` subclasses — defined and consumed within Task 18. Consistent.
- `showMainWindow()` — Task 15 adds it; Task 18 calls it. Consistent.

One correction applied inline: Task 5's summary in the File Structure said "wired in main.rs" for the counter; the `Arc` is created in `DaemonService`'s own constructor (`from_state`) and the guard is taken in `server.rs`, so **main.rs needs no counter change** — only the `peer_addr` change in Task 9. The self-review table above reflects this.
