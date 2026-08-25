# Channels: the daemon-to-daemon communication abstraction

**Status: G1-G8 and the H-track security work (H1-H4) are all done — `daemon/todos.json` tracks A-J are entirely complete.** The `Channel` trait and its vocabulary (`ChannelKind`, `ChannelAddress`, `ChannelMessage`, `ChannelError`) exist in `core/src/channel/mod.rs` exactly as drafted here — replacing the earlier `core::transport::Transport` placeholder. `TcpChannel` and `BluetoothChannel` (Linux-only, `bluetooth` Cargo feature) both implement it against real sockets; `discovery::tcp`/`discovery::bluetooth` produce `DiscoveredPeer` values; `connect_best_available` (`daemon/src/channel/negotiate.rs`) picks between them; `daemon/src/channel/handshake.rs` carries a real pairing handshake over any `Channel`, wired into `DaemonService`'s pairing state machine; `pipeline::send_while_active`/`::receive_and_inject` (G8) stream `InputEvent`s end to end, gated by device-switch state and a replay guard (H4); `NoiseChannel` (H3) authenticates and encrypts every message, bound to each side's `H1` identity; `daemon/src/channel/gate.rs::accept_trusted` (H4) rejects an untrusted peer before any input can flow; `daemon/src/channel/reconnect.rs::maintain_connection` (I1) re-negotiates and reconnects with backoff on a dropped connection. Every one of these is proven by real tests against real sockets — see `daemon/README.md`'s Channels/Security/Auto-reconnect sections for exactly what is and isn't verified in this project's own development environment (no second machine, no Bluetooth adapter). **Not yet wired into `main.rs`**: no incoming-connection accept loop runs in the actual daemon binary, so these remain real, tested, standalone building blocks rather than the thing running when you start `flow-daemon` today — the same honest boundary `daemon/README.md`'s own top status line states. Version **0.1.0**.

## What this is, and what it isn't

A **Channel** is a connection between two Flow daemons, established over whichever medium is actually available: **TCP** (Wi-Fi / local network) or **Bluetooth**. It carries the traffic `docs/product/vision.md` §9-§11 calls "Remote Transport" — pairing handshakes and input events between two machines.

This is **not** the [`docs/contracts/`](../contracts) directory's concern. That directory documents the local IPC boundary between the Flutter UI and *this machine's own* daemon — a different boundary, with a different lifetime and a different trust model (`docs/contracts/README.md` ground rule 3 already draws this line for `core::protocol`; Channels is the same distinction applied to the abstraction that carries that protocol). Nothing in this document changes `docs/contracts/`.

**Why documented separately, but not independently:** Channels is Rust-only today — Flutter never sees a `Channel`. But `DaemonLinkState` (a Flutter-visible contract type) already represents *this machine's* link health, and it's a reasonable future step for the UI to want to know *how* it's connected (e.g. a tray tooltip reading "Connected via Bluetooth"). If that happens, it's a `docs/contracts/data-model.md` change coordinated with this document, not a silent one — this doc uses the same wire-example, versioned, explicit-scope style as `docs/contracts/` specifically so that reconciliation is easy whenever it happens, not because the two are the same contract today.

## The abstraction

```rust
// flow-core::channel

pub enum ChannelKind {
    Tcp,
    Bluetooth,
}

pub enum ChannelAddress {
    Tcp(SocketAddr),
    Bluetooth(BluetoothAddr),
}

pub enum ChannelMessage {
    Input(InputEvent),           // core::protocol — keyboard/mouse events
    Pairing(PairingWireMessage), // pairing handshake, travels over the same channel
    Heartbeat,                   // liveness, used by auto-reconnect (todos.json I1)
}

pub enum ChannelError {
    ConnectionLost,
    Unreachable,
    Serialization(String),
    UnsupportedMedium,
    // ...
}

#[async_trait]
pub trait Channel: Send {
    fn kind(&self) -> ChannelKind;
    async fn send(&mut self, msg: ChannelMessage) -> Result<(), ChannelError>;
    async fn recv(&mut self) -> Result<ChannelMessage, ChannelError>;
    async fn close(&mut self) -> Result<(), ChannelError>;
}
```

Nothing above mentions a WebSocket, a TCP socket, or a Bluetooth crate. That's deliberate: everything downstream of this trait — pairing, input streaming, encryption — is written once, against `Channel`, and works identically regardless of which concrete implementation is underneath. This is the same open/closed discipline the project applies to `InputCapture`/`InputInjector` (per-OS adapters behind one trait) and `Storage` (SQLite behind repository types), now applied to the network boundary.

## Concrete implementations

### `TcpChannel`

Wraps `tokio-tungstenite` — a WebSocket connection over the local network, matching `docs/product/vision.md` §10's stated initial transport ("WebSocket over the local network... simple implementation, easy debugging, cross-platform support, low latency"). `tokio-tungstenite` is used *only* inside this module; nothing else in the daemon imports it directly.

- Higher throughput, lower latency than Bluetooth — the right default whenever both devices share a network.
- Discovery: mDNS or a lightweight UDP broadcast announcing `{name, os, discovery_port}` (`daemon/todos.json` G3).

### `BluetoothChannel`

Wraps a Bluetooth crate over **RFCOMM** (Bluetooth Classic) — an ordered byte stream, the same shape TCP gives, unlike BLE/GATT's small-MTU characteristic model which doesn't fit a continuous input-event stream well. `docs/product/vision.md` §10: "Bluetooth — useful when devices are nearby and the user does not want to depend on the local network."

- **Linux-first.** The `bluer` crate (BlueZ D-Bus bindings) is the only mature, high-level option available to build and test in this project's development environment.
- **macOS/Windows: honest gap, not glossed over.** Neither platform has an equally mature high-level Rust crate for Bluetooth Classic RFCOMM as of this writing (macOS would mean hand-written IOBluetooth bindings; Windows means the WinRT Bluetooth APIs). `daemon/todos.json` G4 is written and cross-compile-checked for both, but flags this gap explicitly in its `buildNote` rather than claiming parity with Linux.
- Discovery: platform Bluetooth scan/advertisement (`daemon/todos.json` G5), producing the same `DiscoveredPeer` shape TCP discovery does.

## Negotiation and fallback

A peer may be reachable over one medium or both. `daemon/todos.json` G6 (`connect_best_available`) picks:

1. **TCP, if the peer is reachable that way.** Higher throughput and lower latency matter most for a continuous mouse-move stream — this is the common case (two machines on the same Wi-Fi/LAN).
2. **Bluetooth, otherwise.** Nearby-but-no-shared-network is exactly the case `vision.md` calls out for Bluetooth.
3. **`ChannelError::Unreachable`** if neither medium can reach the peer — surfaced up as a real error, not a hang.

This is the *only* place in the daemon that knows both concrete `Channel` types exist. Everything above it (pairing, streaming, encryption) only ever holds a `Box<dyn Channel>`.

Auto-reconnect (`daemon/todos.json` I1) re-runs this negotiation on every retry rather than reusing the original medium — the medium that worked when a device was paired (e.g. home Wi-Fi) may not be the one available when the link drops and needs to recover (e.g. away from that network, only Bluetooth-reachable).

## Security

`NoiseChannel` (`daemon/src/channel/noise.rs`, `daemon/todos.json` H3, landed) is a `Channel` that wraps *any other* `Channel`, layering an authenticated [Noise protocol](http://www.noiseprotocol.org/) (`Noise_XX`, via the `snow` crate) session over it. Because it's written against the `Channel` trait rather than a concrete medium, encryption is identical whether the inner channel is `TcpChannel` or `BluetoothChannel` — one implementation, not two. Its Noise handshake itself uses a fresh, per-connection X25519 keypair rather than each side's persisted `H1` ed25519 identity directly (the two are different DH-vs-signing primitives; see `daemon/src/channel/noise.rs`'s own doc comment for why converting between them was deliberately avoided) — the session is instead *bound* to each side's `H1` identity by a signed handshake-transcript exchange immediately after the Noise handshake completes, exposed via `NoiseChannel::peer_identity()`.

Trust (`daemon/todos.json` H2, backed by `P4`) and replay protection (H4, both landed) sit at the same medium-agnostic level: `daemon/src/channel/gate.rs`'s `accept_trusted` checks an incoming connection's Noise-proven identity against the persisted device/trust table before returning a usable channel, and `pipeline::receive_and_inject` drops any `Input` message whose timestamp doesn't strictly increase from the last accepted one — a replay/sequence check reusing the timestamp every `InputEvent` already carries rather than a separate counter. Both run the same way regardless of which `Channel` implementation delivered the connection.

## Wire shape (target, not yet implemented)

`ChannelMessage` is serialized with the same discipline as the local IPC contract — `snake_case`, `serde`-derived, no hand-written framing:

```json
// Input
{ "type": "input", "event": { "type": "keyboard", "event": "keydown", "key": "A", "modifiers": ["Shift"], "timestamp_ms": 123456789 } }

// Pairing (request/response wrap core::pairing types)
{ "type": "pairing", "request": { "device_name": "MacBook", "address": "192.168.1.42:47900" } }

// Heartbeat
{ "type": "heartbeat" }
```

This is intentionally close in spirit to `docs/contracts/daemon-ipc.md`'s envelope (a tagged JSON message, one obvious shape per message kind) without being the same contract — see "What this is, and what it isn't" above.

## Deliberately out of scope for 0.1.0

- **BLE (GATT-based) Bluetooth** — RFCOMM only. GATT's characteristic/MTU model doesn't suit a continuous event stream and would need a materially different `BluetoothChannel` design.
- **Simultaneous multi-path** (using TCP and Bluetooth at once, or roaming between them mid-session without a reconnect) — G6 picks one medium per connection attempt; switching happens only via I1's reconnect-and-renegotiate, not live.
- **>2-device Channel topologies** — matches `docs/contracts/daemon-ipc.md`'s own scope note; a `Channel` is a point-to-point link between exactly two daemons.
- **Exposing Channel/medium state to the Flutter contract** — see "What this is, and what it isn't" above. Tracked as a future `docs/contracts/` change, not part of this document's scope.

## Change history

- **0.1.0** (draft) — initial design: `Channel` trait, `TcpChannel`, `BluetoothChannel`, negotiation/fallback, `NoiseChannel` layering. Not yet implemented; see `daemon/todos.json` track G.
