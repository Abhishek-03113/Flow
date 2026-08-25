# Flutter ↔ Daemon Contracts

This directory is the single source of truth for the boundary between the Flutter control-plane UI and the Rust daemon (`docs/product/vision.md` §9, Communication Between Flutter and Daemon). It exists because the two sides are being built out of order: the UI is being built first against a **mock data source** (`flutter/lib/data/mock_daemon_repository.dart`), and the daemon does not exist yet. These documents are the spec the mock already satisfies and the spec the real daemon must satisfy when its turn comes — not documentation written after the fact.

**Status: draft, mock-only.** Nothing here is implemented by a daemon yet. Contract version `0.1.0` is defined entirely by `MockDaemonRepository`.

## Documents

- [`data-model.md`](./data-model.md) — the entities that cross the boundary: `Device`, `DeviceState`, `DaemonLinkState`, pairing types, settings, switch-key bindings. Dart type next to its JSON wire shape.
- [`daemon-ipc.md`](./daemon-ipc.md) — the RPC commands the UI sends and the state the daemon pushes back: message envelope, command list, event list, error shape, and the pairing/switching state machines.
- [`CHANGELOG.md`](./CHANGELOG.md) — contract version history.

## Ground rules

1. **The Dart abstract class is the contract, not prose.** [`flutter/lib/domain/daemon_repository.dart`](../../flutter/lib/domain/daemon_repository.dart) is the canonical interface. These docs explain it and give it a wire format; if a doc and the code disagree, the code is wrong and should be fixed to match the doc (or vice versa — but they must be reconciled in the same change).
2. **`MockDaemonRepository` must implement the interface exactly**, including timing behavior that's part of the contract (see `daemon-ipc.md` for which delays are meaningful vs. cosmetic). A future real implementation (e.g. `IpcDaemonRepository`) is a drop-in replacement behind the same interface — the UI never imports the mock or a real client directly, only `DaemonRepository`.
3. **This is the local IPC contract, not the wire protocol between two daemons.** `docs/product/vision.md` §11 (Input Event Protocol) and `core/src/protocol` define the keyboard/mouse event format daemons exchange with each other; [`docs/architecture/channels.md`](../architecture/channels.md) defines **Channels**, the abstraction that carries it (a Channel is established between two daemons over TCP or Bluetooth, whichever is available). Both are independent of this directory — a UI never sees a raw `InputEvent` or a `Channel` — and are maintained separately, though in the same documentation style, in case Channel/medium state ever needs to surface into this contract later.
4. **Reuse the Rust vocabulary where the concepts are the same.** `Device.state` in this contract is exactly `flow_core::device::DeviceState` (`core/src/device/mod.rs`) — same variants, same meaning — so the daemon can serialize its existing type directly instead of maintaining a parallel one. Where a concept genuinely doesn't exist on the Rust side yet (pairing candidates, settings, switch-key binding), this contract defines it fresh and `flow-core` should grow to match, not the other way around.
5. **Breaking changes bump the contract version** and get a `CHANGELOG.md` entry. Since nothing consumes this contract but the mock today, breaking it is cheap — but the discipline starts now so it's still true once a daemon depends on it.
