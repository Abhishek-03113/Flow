# flow-daemon

The Rust data-plane daemon for Flow — input capture, injection, device switching, networking, and pairing, running independently of the Flutter UI (`docs/product/vision.md` §8). This directory holds the daemon binary itself; the workspace also includes `flow-core` (`../core`) and `flow-platform` (`../platform`).

**Status: real process, no OS input yet.** `flow-daemon` binds a WebSocket IPC listener on `127.0.0.1:47823`, serves the full `docs/contracts/daemon-ipc.md` contract (backed by `flow-core`'s mock-parity `DaemonService`) over it, and persists to a local SQLite database — tracks A/P/B/C are done. Real OS input capture/injection (track E onward) hasn't landed yet. See [`todos.json`](./todos.json) and [`PLAN.md`](./PLAN.md) for the full build-out plan; a full section-by-section pass (not just this status line) is track J5's job once everything above lands.

## Why this exists separately from the root README

The root [`README.md`](../README.md) covers the whole product and points here for daemon specifics, the same way it points to [`flutter/README.md`](../flutter/README.md) for UI specifics. Once the daemon is real, this file is where "how do I run it, what does it actually do today, what's still stubbed" lives.

## Workspace layout

```
core/       flow-core     — protocol, device, pairing, transport, state, input traits (no OS/transport/UI deps)
daemon/     flow-daemon   — this binary; wires flow-core + flow-platform together, owns the IPC server
platform/   flow-platform — per-OS input adapters (macos/, windows/, linux/) behind flow-core's traits
```

## Building and running

```sh
cargo build --workspace
cargo run -p flow-daemon
```

This binds a WebSocket listener on `127.0.0.1:47823` (`docs/contracts/daemon-ipc.md`'s local IPC contract) and stays running, serving commands and pushing state events, until interrupted (Ctrl-C).

## Testing and linting

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

**Cross-language contract test** (`daemon/todos.json` task D5): `flutter/test/data/ipc_daemon_repository_manual_test.dart` runs the same 13 scenarios `mock_daemon_repository_test.dart` proves against the Dart mock, against a real `flow-daemon` process instead — confirming the Rust and Dart sides agree, not just that each independently passes its own tests. Manual and not part of either `cargo test` or a plain `flutter test`:

```sh
# terminal 1, from the repo root — a fresh HOME so the daemon seeds mock-parity data
HOME=$(mktemp -d) cargo run -p flow-daemon

# terminal 2
cd flutter && flutter test --tags manual --run-skipped test/data/ipc_daemon_repository_manual_test.dart
```

## The contract this daemon implements

[`docs/contracts/`](../docs/contracts) defines the exact interface — commands, state streams, error codes, state machines — that `flutter/lib/data/mock_daemon_repository.dart` implements on the Flutter side. `flow-daemon` is the second, real implementation of that same contract, reachable over local IPC: `flutter/lib/state/repository_providers.dart`'s `daemonRepositoryProvider` can point at it instead of the mock via `--dart-define=FLOW_DAEMON_MODE=ipc`, with **no UI code changing** (`docs/contracts/README.md` ground rule 2; see `flutter/README.md` "Running against a real daemon"). `daemon/todos.json` tracks A-D built exactly this, in order.

## Platform adapters: what's real vs. stubbed

| Platform | Capture | Injection | Verified how |
|---|---|---|---|
| Linux | real (`todos.json` E1, evdev) | real (E2, uinput) | unit-tested (evdev-event ↔ `InputEvent` translation, both directions, pure functions); this session's container has neither `/dev/input` nor `/dev/uinput`, so device discovery, the read loop, the virtual device, and actual keypress capture/injection are unverified beyond compiling — see "Manual verification notes" below |
| macOS | pending (E4) | pending (E5) | written against `core-graphics`, verified with `cargo check --target <apple-target>` only — no Mac hardware in this development environment |
| Windows | pending (E6) | pending (E7) | written against the `windows` crate, verified with `cargo check --target <windows-target>` only — no Windows hardware in this development environment |

Cross-compilation setup instructions land in `todos.json` task J3 once the macOS/Windows adapters exist to check.

## Persistence

Settings, paired devices (which double as the trust store), this daemon's own identity keypair, and a connection history log all live in a single local SQLite database (`rusqlite`, bundled — no system SQLite dependency) under the platform data directory, applied via versioned migrations on startup. Nothing here is derived fresh on every run or held only in memory: a fresh database bootstraps to the same seed data the mock uses (3 devices, defaults), and every subsequent run loads what was actually persisted. `daemon/todos.json` track **P** (`persistence-storage`) builds this, positioned right after the core contract types and ahead of the command service itself, since `DaemonService`'s startup state depends on it.

## Process supervision (not yet implemented)

The daemon is meant to run independent of any UI, ideally started at login/boot and restarted automatically if it crashes (`docs/product/vision.md` principle 7). This phase scaffolds but does not install service unit files (`todos.json` J4: a `launchd` plist, a `systemd` unit, notes for a Windows service wrapper) — actually registering the daemon to auto-start is future work beyond this plan.

## Channels (daemon-to-daemon connectivity)

Daemon-to-daemon connections — the one that actually carries keyboard/mouse input between two machines — go through a single custom abstraction named **Channels**: a `Channel` trait implemented by `TcpChannel` (Wi-Fi/local network, via a wrapped WebSocket) and `BluetoothChannel` (RFCOMM), with a negotiation step that prefers TCP and falls back to Bluetooth when no shared network exists. Full design, wire shape, and explicit scope boundaries: [`docs/architecture/channels.md`](../docs/architecture/channels.md). This is a separate document from `docs/contracts/` on purpose — Channels is daemon<->daemon, the contracts directory is Flutter<->daemon — kept in the same documentation style so the two are easy to reconcile if Channel state ever needs to surface into the UI later. `daemon/todos.json` track **G** builds this.

Every third-party dependency that does real I/O (SQLite, WebSockets, Bluetooth, Noise encryption, OS input APIs) is wrapped behind a project-owned trait or type before anything else in the daemon depends on it — see `todos.json`'s `architecturalPrinciples.wrapThirdPartyDependencies` for the explicit rule and the module-by-module list of what wraps what.

## Security posture during this phase

The local IPC channel (Flutter <-> this daemon) is bound to `127.0.0.1` only and assumes it's reachable solely by the local user, per `docs/contracts/README.md`'s scope note — it does not carry its own authentication in v0.1. The daemon-to-daemon Channel (once track G/H land) is the one that actually carries sensitive keyboard/mouse data, and is where real device identity, trust, and Noise encryption (`docs/product/vision.md` §17) apply — uniformly across both TCP and Bluetooth, since encryption wraps the `Channel` trait rather than either medium specifically.

## Manual verification notes

Several tasks in `todos.json` (E1-E3, E4-E7, G4, I4) can only be fully verified with real input devices, a second machine, or platform hardware this development environment doesn't have. Each such task's acceptance criteria says explicitly what was verified automatically (unit tests on pure translation logic, `cargo check` for cross-compiled platforms, integration tests against synthetic events) versus what still needs a human with the actual hardware to confirm. This section will grow with concrete "how to manually verify" steps as those tasks land.

### E1: Linux capture via evdev

`platform/src/linux/capture.rs` (`LinuxInputCapture`) discovers keyboard/mouse-capable nodes via `evdev::enumerate()` (`discovery.rs`), reads them non-blocking on a dedicated thread, and translates each event through the pure `EventTranslator` (`translate.rs`) before sending it down an `mpsc::Sender<InputEvent>` supplied at construction. This session's container has no `/dev/input` or `/dev/uinput` at all (confirmed via `ls /dev/input`, `ls /dev/uinput` — both "No such file or directory", running as root), so nothing beyond `cargo build -p flow-platform` and the translation unit tests could be exercised here. On a machine with real input devices and the `input` group (or root):

```sh
# confirm device nodes and permissions
ls -la /dev/input/event*

# a minimal manual check: construct a LinuxInputCapture with an mpsc channel,
# call start(), type/click, and print what arrives on the receiver — e.g. via
# the E3 CLI harness once it lands, or a throwaway `cargo run --example`
```

What to look for: `start()` returns `Ok(())` (not `NotFound`, which means no qualifying device was found — check the account is in the `input` group), and keypresses/clicks/scrolls on the physical device show up as the expected `InputEvent` variants on the channel, including the correct `modifiers` list for chorded keys (e.g. Shift+A). `stop()` should return once the read thread's next idle-poll notices the stop flag (≤5ms).

### E2: Linux injection via uinput

`platform/src/linux/injector.rs` (`LinuxInputInjector`) creates a virtual device via `evdev::uinput::VirtualDeviceBuilder`, declaring the full `EV_KEY` range (`input-event-codes.h`'s `0..=KEY_MAX`, so any key name `translate::key_name` can produce is injectable, not just letters) plus `REL_X`/`REL_Y`/`REL_WHEEL`/`REL_HWHEEL`, and replays `InputEvent`s onto it through the pure `inject_translate::to_uinput_events` function (the reverse of E1's `EventTranslator`). `LinuxInputInjector::new()` opens `/dev/uinput`, which this container doesn't have (confirmed via `ls /dev/uinput` — "No such file or directory"), so beyond `cargo build`/`test`/`clippy`/`fmt`, nothing about the virtual device or real injected input was exercised here. On a machine with `/dev/uinput` and the `uinput` kernel module loaded (`modprobe uinput`), and write access to it (the `uinput` udev group, or root):

```sh
# confirm the module and device node
lsmod | grep uinput
ls -la /dev/uinput

# a minimal manual check: construct a LinuxInputInjector, call inject() with
# a few InputEvents, and confirm the virtual device shows up and the events
# land — e.g. via `evtest` on the new /dev/input/eventN it creates, or the
# E3 CLI harness once it lands
```

What to look for: `LinuxInputInjector::new()` returns `Ok(_)` (a `PermissionDenied` means the account isn't in the right group), a new `/dev/input/eventN` node named "Flow Virtual Input" appears while the injector is alive, and `evtest` (or the E3 harness) shows the expected `EV_KEY`/`EV_REL` events with correct codes and values for each `inject()` call, including that a `MouseEvent::Move`/`Scroll`'s two axes land as one atomic `SYN_REPORT`-terminated batch rather than two separate reports.
