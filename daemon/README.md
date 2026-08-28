# flow-daemon

The Rust data-plane daemon for Flow — input capture, injection, device switching, networking, and pairing, running independently of the Flutter UI (`docs/product/vision.md` §8). This directory holds the daemon binary itself; the workspace also includes `flow-core` (`../core`) and `flow-platform` (`../platform`).

**Status: every task in [`todos.json`](./todos.json) (tracks A-J) is done, and the daemon-to-daemon pieces those tasks built as standalone, tested modules are now spliced into `main.rs` itself** (a later integration pass, not part of tracks A-J's own scope). In order of what actually runs: `flow-daemon` binds a WebSocket IPC listener on `127.0.0.1:47823`, serves the full `docs/contracts/daemon-ipc.md` contract (backed by `flow-core`'s mock-parity `DaemonService`) over it, and persists settings/devices/identity/history to a local SQLite database (tracks A/P/B/C). `flow-platform` has real per-OS capture/injection for Linux (evdev/uinput), macOS (`CGEventTap`/`CGEventPost`), and Windows (`SetWindowsHookEx`/`SendInput`) — track E — with the local switch-key hotkey (F1-F3) wired to actually flip the active device without needing the UI running at all. `main.rs` also binds a UDP discovery socket and a second TCP listener (the "peer channel," distinct from the IPC port) on startup: two independently-*started* `flow-daemon` processes on the same network now discover each other, negotiate a medium, complete a real pairing handshake, and stream keyboard/mouse input to whichever device is currently `Active` (track G, plus the integration pass's own `main.rs` wiring — see "Daemon-to-daemon wiring in `main.rs`" below). On Linux the sending machine also suppresses that input locally, so it goes *to* the active device rather than to both; on macOS/Windows that half isn't implemented yet (see the bullet above). That connection is authenticated (a Noise session bound to each side's persisted ed25519 identity) and gated against a trust store before any input can flow, with a replay guard on the receiving side (track H); a dropped connection is rediscovered and reconnected via discovery's periodic re-announce (not `channel::reconnect::maintain_connection`'s own backoff loop — see below), every state-changing command is traced, and a panic in one connection handler can't take down another (track I). Track J closes the loop: the workspace lints clean, the full test suite and cross-compile checks are documented and verified, service-unit scaffolding exists for macOS/Linux, and this file (see the reconciliation note in its final section) has been checked against the actual code rather than left aspirational.

**What "done" does not mean here**, stated plainly rather than left to be inferred:

- **Local input suppression only works on Linux.** Forwarding input to a peer is only half of remote control; the sending machine also has to stop its own OS from processing those same keystrokes, or every keypress lands on both computers. `InputCapture::set_suppress_local` is that half. Linux implements it for real (an exclusive `EVIOCGRAB` over every captured device); **macOS and Windows return `SuppressionUnsupported` and do nothing** — deliberately erroring rather than silently succeeding, so the daemon logs a warning instead of pretending. Until those land, cross-device control on macOS/Windows types into both machines at once. See "Local input suppression" below.
- **Bluetooth is not wired into `main.rs` at all.** `channel::bluetooth`/`discovery::bluetooth` remain standalone, tested building blocks behind the `bluetooth` Cargo feature — nothing calls `discovery::bluetooth::scan` or feeds its results anywhere. Only the TCP medium is live.
- **Nothing here has run against a real second machine.** Everything beyond Linux capture/injection is verified by cross-compilation and unit tests only; this project's own development container has neither a second machine nor the OS-native input hardware (`/dev/input`, `/dev/uinput`, a Bluetooth adapter). The `main.rs` wiring is verified by its own unit/integration tests plus manually running the built binary here (all three listeners come up, the hotkey runner degrades gracefully with no `/dev/input`) — not by two daemons on two physical computers actually driving each other. Treat "two daemons discover, pair, and stream" as *implemented and unit-tested*, not as *demonstrated on real hardware*.
- **Pairing has no confirmation prompt.** An incoming pairing request is accepted only while the local user has a pairing session open (see "Pairing consent window" below), but within that window it's first-come-first-served, with no code to compare between the two devices.

See [`todos.json`](./todos.json) for the exact task-by-task record (every task's `status`/`buildNote` explains what was built, what was verified how, and any deviation from the original plan) and [`PLAN.md`](./PLAN.md) for the original track-by-track build-out plan this executed.

## Why this exists separately from the root README

The root [`README.md`](../README.md) covers the whole product and points here for daemon specifics, the same way it points to [`flutter/README.md`](../flutter/README.md) for UI specifics. Once the daemon is real, this file is where "how do I run it, what does it actually do today, what's still stubbed" lives.

## Workspace layout

```
core/       flow-core     — protocol, device, pairing, channel, state, input traits (no OS/transport/UI deps)
daemon/     flow-daemon   — this binary; wires flow-core + flow-platform together, owns the IPC server
platform/   flow-platform — per-OS input adapters (macos/, windows/, linux/) behind flow-core's traits
```

## Building and running

```sh
cargo build --workspace
cargo run -p flow-daemon
```

This binds a WebSocket listener on `127.0.0.1:47823` (`docs/contracts/daemon-ipc.md`'s local IPC contract) and stays running, serving commands and pushing state events, until interrupted (Ctrl-C).

A startup failure that can't be recovered from — the IPC port already in use, an unwritable data directory — now exits non-zero with a one-line `flow-daemon: ...` message on stderr instead of a panic backtrace.

### Running a second instance on one machine

For local two-daemon testing (discovery + pairing without a second physical computer), four environment variables relocate everything the two instances would otherwise contend for:

```sh
FLOW_DEVICE_NAME="Flow-B" \
FLOW_DATA_DIR=/tmp/flow-b \
FLOW_IPC_PORT=47833 \
FLOW_IPC_TOKEN_PATH=/tmp/flow-b/ipc.token \
cargo run -p flow-daemon
```

- `FLOW_IPC_PORT` — the `127.0.0.1` IPC listener port (default `47823`). The Flutter client must be pointed at the same value with `--dart-define=FLOW_IPC_PORT`.
- `FLOW_DATA_DIR` — the directory holding `flow.db` (and with it this instance's own persisted ed25519 identity). Default: the platform data dir.
- `FLOW_IPC_TOKEN_PATH` — the IPC auth token file (default `~/.flow/ipc.token`). Pass the same path to the Flutter client with `--dart-define=FLOW_IPC_TOKEN_PATH`.
- `FLOW_DEVICE_NAME` — overrides the local device's display name (default: the machine hostname), so two instances on one box aren't both called the same thing in the UI.

The UDP discovery port (`47824`) is deliberately *not* overridable — both instances bind it with `SO_REUSEADDR` so they still hear each other's broadcast announces. None of these variables have any effect on a normal single-instance deployment when left unset.

## Testing and linting

The default (no extra features, native target) sweep — this is the bar every commit in this repo is expected to clear, and what `daemon/todos.json` J1 formalizes:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

`flow-daemon` also has a `bluetooth` Cargo feature (Linux-only — `channel::bluetooth`, `discovery::bluetooth`, track G4/G5) that the default sweep above doesn't touch at all, so check it too:

```sh
cargo test -p flow-daemon --features bluetooth
cargo clippy -p flow-daemon --features bluetooth --all-targets -- -D warnings
```

There is currently no `#[allow(...)]` anywhere in `core`/`daemon`/`platform` — every lint the workspace clippy configuration flags is either fixed outright or the code is restructured to avoid it, not suppressed. If a future change genuinely needs one, justify it with a comment at the point of use (a workspace-wide `clippy.toml` blanket-allowance isn't warranted today since nothing needs one yet — adding one speculatively would be lint config for a problem that doesn't exist).

**Cross-compilation checks** (`daemon/todos.json` J2/J3) — `flow-core` and `flow-platform` (the two crates E1-E7's platform adapters live in) are checked, not built, against the macOS and Windows targets, proving the code at least type-checks/compiles for platforms this container can't run:

```sh
rustup target add x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc
cargo check -p flow-core -p flow-platform --target x86_64-apple-darwin
cargo check -p flow-core -p flow-platform --target aarch64-apple-darwin
cargo check -p flow-core -p flow-platform --target x86_64-pc-windows-msvc
cargo clippy -p flow-core -p flow-platform --target x86_64-apple-darwin --all-targets -- -D warnings
cargo clippy -p flow-core -p flow-platform --target x86_64-pc-windows-msvc --all-targets -- -D warnings
```

**Deliberately scoped to `flow-core`/`flow-platform`, not `--workspace`.** `cargo check --workspace --target <macos-or-windows>` (including `flow-daemon`) genuinely fails in this development environment — verified directly, not assumed: `rusqlite`'s `bundled` feature compiles SQLite's C source as part of its build script, which needs a real per-target C toolchain (a macOS cross-`cc` with the right `-arch`/`-mmacosx-version-min` support, or `lib.exe` from an MSVC toolchain for the Windows target) that this container doesn't have and installing isn't reasonably in scope here. This is a pre-existing gap in the *development environment*, not evidence `flow-daemon` itself can't compile on macOS/Windows — a machine with Xcode or a Windows MSVC toolchain installed wouldn't hit it. `flow-core`/`flow-platform` have no such dependency (no bundled C code), which is exactly why E1-E7 scoped their own cross-compile verification to those two crates in the first place, not the whole workspace — this note makes that scoping decision explicit rather than leaving it implicit.

**Cross-language contract test** (`daemon/todos.json` task D5): `flutter/test/data/ipc_daemon_repository_manual_test.dart` runs the same 13 scenarios `mock_daemon_repository_test.dart` proves against the Dart mock, against a real `flow-daemon` process instead — confirming the Rust and Dart sides agree, not just that each independently passes its own tests. Manual and not part of either `cargo test` or a plain `flutter test`:

```sh
# both terminals — a fresh, *shared* HOME so both processes agree on
# where ~/.flow/ipc.token lives (the daemon writes it there on startup,
# and IpcDaemonRepository reads it from the identical path to
# authenticate its connection); FLOW_DAEMON_SEED_MOCK_PARITY makes this
# one daemon instance seed the mock-parity fixture the test asserts on
# instead of real (empty) production state — never set this outside a
# test run like this one
export HOME=$(mktemp -d)

# terminal 1, from the repo root
FLOW_DAEMON_SEED_MOCK_PARITY=1 cargo run -p flow-daemon

# terminal 2 (same HOME as terminal 1 — export it there too, or run both
# commands from one shell that already has it exported)
cd flutter && flutter test --tags manual --run-skipped test/data/ipc_daemon_repository_manual_test.dart
```

**Full daemon-to-UI end-to-end test:** `flutter/test/e2e/daemon_ui_flow_e2e_test.dart` goes further than the contract test above — it drives the actual production Flutter screens (tray popover, settings, onboarding) through real taps against a real `flow-daemon`, and unlike the contract test it starts and stops that daemon itself (no manually-started process, no shared-`HOME` two-terminal dance). Tagged `e2e`, automated, single command:

```sh
cd flutter && flutter test --tags e2e --run-skipped test/e2e/daemon_ui_flow_e2e_test.dart
```

See `docs/testing/manual-testing-strategy.md`'s Tier 0 for what this automates.

## The contract this daemon implements

[`docs/contracts/`](../docs/contracts) defines the exact interface — commands, state streams, error codes, state machines — that `flutter/lib/data/mock_daemon_repository.dart` implements on the Flutter side. `flow-daemon` is the second, real implementation of that same contract, reachable over local IPC: `flutter/lib/state/repository_providers.dart`'s `daemonRepositoryProvider` can point at it instead of the mock via `--dart-define=FLOW_DAEMON_MODE=ipc`, with **no UI code changing** (`docs/contracts/README.md` ground rule 2; see `flutter/README.md` "Running against a real daemon"). `daemon/todos.json` tracks A-D built exactly this, in order.

## Platform adapters: what's real vs. stubbed

| Platform | Capture | Injection | Verified how |
|---|---|---|---|
| Linux | real (`todos.json` E1, evdev) | real (E2, uinput) | unit-tested (evdev-event ↔ `InputEvent` translation, both directions, pure functions); this session's container has neither `/dev/input` nor `/dev/uinput`, so device discovery, the read loop, the virtual device, and actual keypress capture/injection are unverified beyond compiling — see "Manual verification notes" below |
| macOS | real (E4, `CGEventTap`) | real (E5, `CGEventPost`) | cross-compile checked (`cargo check -p flow-platform --target x86_64-apple-darwin` and `--target aarch64-apple-darwin`) plus `clippy`/`fmt` on both; the `CGEvent <-> InputEvent` translation has unit tests in both directions, but they need macOS to execute — this Linux container can only compile-check them, not run them, and there's no Mac hardware here at all — see "Manual verification notes" below |
| Windows | real (E6, `WH_KEYBOARD_LL`/`WH_MOUSE_LL`) | real (E7, `SendInput`) | cross-compile checked (`cargo check`/`clippy --all-targets` for `flow-platform` on `x86_64-pc-windows-msvc`) plus `fmt`; the `InputEvent <-> INPUT` translation has unit tests in both directions, but no Windows hardware exists here to run them — see "Manual verification notes" below |

Cross-compilation setup instructions (exact `rustup target add`/`cargo check` invocations) are documented in "Testing and linting" above (`daemon/todos.json` task J3).

## Switch-key hotkey

`daemon/src/hotkey/` detects the configured switch-key combination directly from the platform's real input capture and triggers a device switch **without any IPC client connected**, per `vision.md` §8 ("Daemon Works without UI"). Two pieces:

- `hotkey::SwitchKeyMatcher` (`mod.rs`, track F1) — a pure, platform-neutral matcher fed one `InputEvent` at a time, detecting when the current `FlowSettings.switch_key` binding's tokens are all satisfied simultaneously. Fully unit-tested in this environment (no hardware needed).
- `hotkey::runner::spawn` (`runner.rs`, track F2) — starts `flow_platform::DefaultInputCapture` (whichever real per-OS adapter this binary was built for), bridges its event stream through the matcher, and calls `DaemonService::switch_active_device_local()` on a match — a separate, error-free path from the IPC `switch_active_device` command (track C), since a raw key press has no "requester" to reject with an error. Spawned alongside the IPC listener and history logger in `main.rs`.

**The hotkey runner degrades gracefully, not fatally**, when the platform adapter can't start (no capturable device, missing permission): it logs a warning and the daemon keeps serving IPC normally without it. Confirmed in this container, which has no `/dev/input` at all:

```
WARN flow_daemon::hotkey::runner: hotkey runner not started: input capture failed: Custom { kind: NotFound, error: "no keyboard- or mouse-capable /dev/input device found" }
INFO flow_daemon: flow-daemon listening on 127.0.0.1:47823
```

On a machine with a real capturable device, the switch key (Scroll Lock by default) actually advances the active device — cycling in device-id order starting just after whichever device is currently active, wrapping around, and skipping `Disconnected` devices. A 500ms debounce (`hotkey::debounce::SwitchDebouncer`, track F3) collapses key-repeat or a noisy multi-key combo release into exactly one switch per press — this daemon reads raw key events directly, unlike the Dart mock, whose debounce is a UI-visible concern per `docs/contracts/daemon-ipc.md`'s "Switching" section, not a wire-level one.

## Persistence

Settings, paired devices (which double as the trust store), this daemon's own identity keypair, and a connection history log all live in a single local SQLite database (`rusqlite`, bundled — no system SQLite dependency) under the platform data directory, applied via versioned migrations on startup. Nothing here is derived fresh on every run or held only in memory: a fresh database bootstraps to just this machine's own real local device (see "Removing mock runtime data" below — settings still get their real defaults), and every subsequent run loads what was actually persisted. `daemon/todos.json` track **P** (`persistence-storage`) builds this, positioned right after the core contract types and ahead of the command service itself, since `DaemonService`'s startup state depends on it.

### Removing mock runtime data

`ServiceState` (`daemon/src/service/mod.rs`) has two loaders, deliberately kept separate rather than branching on "is the database empty":

- **`ServiceState::from_storage`** — what a real `flow-daemon` process actually uses (`DaemonService::new`, `main.rs`'s only call site). A fresh (empty) database seeds *only* this machine's own real device record — real hostname (`hostname` crate), real compiled-target OS, `Active`, not removable — never fake remote devices, never fake pairing candidates (`candidates_pool` starts empty; real candidates only ever come from `note_discovered_peer`, track G7's live discovery), and `link_state` starts `Disconnected` rather than a claimed `Connected` — see "Link health" below for the real transitions that earn `Connected`.
- **`ServiceState::seeded_for_test`** — the exact mock-parity 3-device/2-candidate/`Connected` fixture `MockDaemonRepository` also ships. Every daemon test predating this split was written against it and asserts on its specific device ids/candidates/local device name ("MacBook"), so tests opt into it explicitly via `DaemonService::new_seeded_for_test` instead of it being an implicit fallback for an empty database.

`main.rs` also honors `FLOW_DAEMON_SEED_MOCK_PARITY` (any non-empty value) to run a real `flow-daemon` *process* against the mock-parity fixture instead of real state — the one legitimate reason being `flutter/test/data/ipc_daemon_repository_manual_test.dart`'s cross-language contract test, which needs a real process to assert its mock-parity expectations against ("Cross-language contract test" above). A real deployment never sets this, and nothing in this codebase sets it automatically.

`service::tests::production_init_never_seeds_mock_parity_data` is the regression guard: it calls the real `DaemonService::new` and fails if production ever seeds more than the one real local device, or ever claims `Connected` before a real peer link exists — the test that would catch someone "fixing a bug" by pointing `from_storage` back at `seeded_for_test`.

### Device identity (track H1)

`storage::identity_repo` (P6) persists 64 bytes across restarts but, by its own doc comment, deliberately doesn't interpret them cryptographically — "the same bytes come back every time," nothing more. `daemon/src/identity/mod.rs`'s `DeviceIdentity` (H1) is what turns those bytes into an actual, verifiable [ed25519](https://docs.rs/ed25519-dalek) keypair: it treats the repo's persisted `private_key` column as an `ed25519-dalek` `SigningKey` seed (both happen to be 32 bytes) and derives the public key from it — deliberately *not* trusting the repo's own `public_key` column, which is filled with independently-random bytes that bear no cryptographic relationship to the private key at all (that mismatch is expected and harmless: nothing reads that column once `DeviceIdentity` exists). `load_or_generate(storage)` gives the same keypair every call against the same database; `sign`/the free `verify` function round-trip a real signature, which is what proves this is a mathematically valid keypair and not just 32 arbitrary bytes reinterpreted. `public_key_bytes()` is the form `H2`'s trust gate and `P4`'s device repository actually compare/store.

## Process supervision (track I3; unit files not yet installed — J4)

The daemon is meant to run independent of any UI, ideally started at login/boot and restarted automatically if it crashes (`docs/product/vision.md` principle 1: "Invisible by default — the user should forget the software is running," and principle 7). This section documents the intended supervision model per-OS; `daemon/todos.json` J4 is where the actual unit files get written and installed — deliberately OS-integration/packaging scope, out of this task's own reach.

**Within the process, panics are already isolated per-task**, which is what makes OS-level auto-restart a reasonable strategy in the first place (an isolated bug taking down one connection is recoverable without restarting the whole daemon; only a genuinely fatal condition should ever need the OS supervisor to step in). Every long-running unit of work — each IPC connection (`main.rs`'s accept loop spawns one `tokio::spawn` per connection), the hotkey runner, `history_logger` — runs as its own `tokio::spawn`ed task, and Tokio's runtime catches a panic inside one task and reports it only through that task's own `JoinHandle`, never propagating it to the runtime or any other task. `ipc::server::tests::a_panicking_connection_handler_does_not_affect_a_concurrent_one` is this task's regression test: a task made to panic deliberately reports `Err` on its own `JoinHandle` while a second, real, concurrently-running connection completes normally and unaffected — proving this holds for the exact spawn shape the daemon's own code uses, not just as an abstract Tokio property. The hotkey runner uses the identical `tokio::spawn(async move { ... })` primitive that regression test exercises, so the same guarantee applies to it for the same reason; it isn't separately re-tested here because doing so for real needs a capturable input device this project's own development container doesn't have (see "E1: Linux capture via evdev" below) — the same gap that already makes `hotkey::runner::spawn` return `None` rather than panic in this environment.

Per-task isolation only covers bugs inside a spawned task's own body — it says nothing about the process itself getting killed (OOM, a signal, a `panic = "abort"` build profile, a segfault in an `unsafe` FFI call like the platform adapters' OS API bindings). That's what OS-level supervision is *for*, and why it matters even though in-process isolation already handles the common case:

- **macOS**: a `launchd` `LaunchAgent` plist (user-level, since this runs per-login-session alongside a UI, not as a root daemon) with `KeepAlive` set (or `KeepAlive.SuccessfulExit = false`, restarting only on an unexpected exit/crash, not a clean `Ctrl-C`/quit) and `RunAtLoad` for start-at-login.
- **Linux**: a `systemd` user unit (`~/.config/systemd/user/flow-daemon.service`) with `Restart=on-failure` and enabled via `systemctl --user enable`, so a login-session systemd instance manages it the same way `launchd` does on macOS.
- **Windows**: a Windows Service (`SERVICE_AUTO_START` + a failure-actions policy configured via `sc.exe failure`/the Service Control Manager) is the closest equivalent, though it runs in Session 0 rather than a user session, which has real implications for anything requiring interactive desktop access (`docs/product/vision.md`'s input-capture APIs are user-session-scoped) — this is flagged as a genuine open design question for J4 to resolve, not glossed over here as if a Windows Service were a drop-in equivalent to the macOS/Linux user-level approaches.

None of the three above are installed by this phase — `flow-daemon` today is a plain binary you run manually (`cargo run -p flow-daemon` or the built executable), with no OS registration at all. That gap is what a future, full implementation of J4 closes.

**Scaffolding exists for the first two** (`daemon/todos.json` J4, stubs only — not installed, activated, or referenced by any build/install step in this repo): [`packaging/launchd/com.flow.daemon.plist`](../packaging/launchd/com.flow.daemon.plist) and [`packaging/systemd/flow-daemon.service`](../packaging/systemd/flow-daemon.service) implement exactly the macOS/Linux notes above, with the placeholder install path/log paths clearly marked as needing to be filled in for a real machine. Both were validated for real: the systemd unit passes `systemd-analyze verify` cleanly once its placeholder `ExecStart` path is pointed at any real executable (the verify tool otherwise, correctly, reports the placeholder path doesn't exist — that's expected for a stub, not a syntax error); the plist parses cleanly under Python's `plistlib` (this container has no `plutil`, a macOS-only tool, to run the canonical check with — `plutil -lint packaging/launchd/com.flow.daemon.plist` is the real validation command once on a Mac). [`packaging/windows/README.md`](../packaging/windows/README.md) is the Windows counterpart: since a real Windows Service runs in Session 0 (no desktop, no access to the interactive-session-scoped `SetWindowsHookEx`/`SendInput` APIs `flow-platform`'s Windows adapters use), there's no literal unit-file equivalent to stub — that document instead lays out the two viable implementation approaches (a wrapper like NSSM/WinSW, or a real Win32 service via the `windows` crate) and the Session 0 isolation problem as an explicit open design question for whoever picks this up next, rather than picking one silently.

## Channels (daemon-to-daemon connectivity)

Daemon-to-daemon connections — the one that actually carries keyboard/mouse input between two machines — go through a single custom abstraction named **Channels**: a `Channel` trait (`flow_core::channel`, track G1) implemented by `TcpChannel` (`daemon/src/channel/tcp.rs`, track G2 — Wi-Fi/local network, via a wrapped WebSocket) and `BluetoothChannel` (`daemon/src/channel/bluetooth.rs`, track G4 — RFCOMM/Bluetooth Classic, via a wrapped `bluer`), with `connect_best_available` (`daemon/src/channel/negotiate.rs`, track G6) picking which one to actually use for a given peer: TCP whenever reachable, Bluetooth otherwise. Full design, wire shape, and explicit scope boundaries: [`docs/architecture/channels.md`](../docs/architecture/channels.md). This is a separate document from `docs/contracts/` on purpose — Channels is daemon<->daemon, the contracts directory is Flutter<->daemon — kept in the same documentation style so the two are easy to reconcile if Channel state ever needs to surface into the UI later. `daemon/todos.json` track **G** builds this.

`daemon/src/discovery/tcp.rs` (track G3) is how a peer's `TcpChannel` address gets found in the first place: a UDP broadcast announce/listen loop (`DiscoveryService`, port `47824`) advertising `{name, os, channel_port}`, producing `DiscoveredPeer { name, os, address: ChannelAddress::Tcp(..) }` values. This module's own tests exercise the send/receive/parse code path via directed loopback (`127.0.0.1:<peer's bound port>`) rather than relying on genuine broadcast fan-out — this session's own sandboxed network rewrote broadcast packets unpredictably when tested directly (a real send to `255.255.255.255` came back with its source address rewritten to a `192.0.2.x` TEST-NET address), and containerized/sandboxed networks often don't support broadcast fan-out reliably anyway. `daemon/src/discovery/bluetooth.rs` (track G5) is the Bluetooth counterpart — see its own subsection below.

**Why two real daemons on the same LAN can each be running and listening and still never discover each other**, and what `broadcast_destinations()` does about the first of these:

1. **Multi-homed hosts send the limited broadcast out the wrong interface.** A single announce to the limited broadcast address `255.255.255.255` leaves the OS routing table to pick *one* outgoing interface, same as any other destination. A real dev machine commonly has more than one active interface — a VPN client, Docker Desktop, Hyper-V/WSL's virtual switch — each adding its own adapter and route; if the routing table doesn't prefer the actual LAN adapter, the announce leaves via the wrong interface and never reaches the peer, silently, with nothing logged as an error on either side. `DiscoveryService::broadcast_destinations()` fixes this at the source rather than requiring correct routing: it enumerates this host's own IPv4 interfaces (`if_addrs`) and sends the announce to each one's subnet-directed broadcast address (e.g. `192.168.1.255`) explicitly, alongside the limited-broadcast fallback for interface enumeration failing or a genuinely single-NIC host. `spawn_discovery` logs the resolved destination list at startup (`flow-daemon announcing to [...] every 5s`) — worth checking first when discovery isn't working, since an empty or unexpected list points straight at the interface, not the protocol.
2. **A host firewall drops the inbound UDP silently.** Windows Defender Firewall in particular blocks unsolicited inbound UDP to an app it hasn't already allowed; running an unsigned dev build for the first time normally prompts to allow it on Private/Public networks, but if that prompt is dismissed (or the daemon runs headless, with no one there to answer it), every inbound announce on port `47824` is dropped before `recv_one` ever sees it — again with no error surfaced anywhere, since from the socket's point of view no packet arrived at all. Check the Windows Firewall allowed-apps list (or add an explicit inbound rule for UDP `47824`) if a Windows daemon never sees an announce even though `flow-daemon discovery listening on 0.0.0.0:47824` logged at startup. macOS's Application Firewall (System Settings → Network → Firewall) can do the same if "Block all incoming connections" is enabled.
3. **The two hosts aren't actually on the same broadcast domain.** Subnet-directed broadcast (point 1's fix) only reaches hosts on that same L2 segment — a guest Wi-Fi network with client isolation enabled, or the two machines genuinely being on different subnets/VLANs, means no amount of retrying the send fixes it; that's a network-topology problem outside what this protocol can work around, and mDNS wouldn't fare any better here either.

`daemon/src/channel/handshake.rs` (track G7) is the actual pairing handshake — the `PairingRequest`/`PairingDecision` exchange carried as `ChannelMessage::Pairing` frames — and `DaemonService::pair_with_candidate`/`::note_discovered_peer`/`::accept_pairing_request` wire it into the existing pairing state machine (track B4), so two daemons *can* now find and pair with each other for real over a `TcpChannel` (Bluetooth too, once something feeds `discovery::bluetooth`'s output into `note_discovered_peer` the same way). `daemon/tests/pairing_over_channel.rs` proves it end to end against real loopback sockets. `DiscoveryService::spawn_listener` and a production-side incoming-`TcpListener`/`bluer::rfcomm::Listener` accept loop aren't wired into `main.rs` yet — see the G7 subsection below for exactly what that leaves out.

### BluetoothChannel (track G4)

`BluetoothChannel` (`daemon/src/channel/bluetooth.rs`) implements `Channel` over RFCOMM (Bluetooth Classic — an ordered byte stream, matching the shape `TcpChannel` gives, unlike GATT/BLE's small-MTU characteristic model), wrapping the [`bluer`](https://docs.rs/bluer) crate (BlueZ D-Bus bindings) — confined to this one module per the wrap-third-party-dependencies rule. It connects/listens on a fixed RFCOMM channel number (`RFCOMM_CHANNEL = 5`, an arbitrary but fixed choice within RFCOMM's valid 1-30 range); real deployment would instead negotiate this via an SDP service record, deliberately deferred past this task's scope. Since RFCOMM has no message framing of its own (unlike `TcpChannel`'s WebSocket, which already frames text messages), this module adds a 4-byte big-endian length prefix ahead of each JSON-encoded `ChannelMessage`, mirroring what a WebSocket frame gives for free.

**Linux-only**, and opt-in: gated behind the `bluetooth` Cargo feature (`cargo build -p flow-daemon --features bluetooth`), not built by default, because `bluer` wraps BlueZ, which only exists on Linux. There is no equally mature high-level Bluetooth Classic RFCOMM crate for macOS (would mean hand-written `IOBluetooth` bindings) or Windows (the WinRT Bluetooth APIs) as of this writing — an honest gap this crate doesn't attempt to paper over, same as `flow-platform`'s E4-E7 platform caveats. Because `bluer` cannot target macOS/Windows at all (it isn't a matter of an unimplemented feature — the crate itself doesn't compile there), there's no `cargo check --target <apple-target>`/`<windows-target>` to run for this feature the way E4-E7 could cross-compile-check; this is a documented scope gap, not an unverified claim.

This session's own container has **no Bluetooth support at the kernel level at all** — confirmed directly (not assumed): creating a raw `AF_BLUETOOTH`/`SOCK_STREAM`/`BTPROTO_RFCOMM` socket here fails with `EAFNOSUPPORT` ("Address family not supported by protocol"), and there's no `/sys/class/bluetooth` and no `hciconfig` binary — independent of whether `bluetoothd` or a real adapter is present. So while `parse_address` (converting between `flow_core::channel::BluetoothAddr` and `bluer::Address`) is genuinely unit-tested here, the actual RFCOMM `send`/`recv`/`close` implementation could only be built to compile and reviewed by hand in this environment, not exercised against a real socket. A full loopback round-trip test exists (`a_hand_crafted_heartbeat_round_trips_over_a_local_loopback_rfcomm_pair`) but is marked `#[ignore]` with an explicit reason string; run it manually on a Linux machine with a real Bluetooth adapter and `bluetoothd` running:

```sh
cargo test -p flow-daemon --features bluetooth --lib channel::bluetooth -- --ignored
```

### Bluetooth peer discovery/advertisement (track G5)

`daemon/src/discovery/bluetooth.rs` is the Bluetooth counterpart to `discovery::tcp`, producing the same `DiscoveredPeer` shape (now hoisted up into `daemon/src/discovery/mod.rs` since both discovery mediums produce it) so G6's channel negotiation can treat either discovery source uniformly. Bluetooth Classic has no equivalent of a UDP broadcast payload — a nearby device only ever surfaces its address and its self-reported *alias* (a short display name) via BlueZ's inquiry scan — so instead of a dedicated wire packet like `discovery::tcp::Announce`, this module encodes the peer's name and OS into the adapter's alias string itself, prefixed (`flow:{"name":"...","os":"..."}`) so it's recognizable as a Flow daemon rather than any other nearby Bluetooth device. `advertise()` sets a discoverable adapter's alias to this encoding; `scan()` spawns a background loop over `Adapter::discover_devices()` that decodes each newly-seen device's alias and forwards a `DiscoveredPeer` for any that match, silently skipping devices that don't (the same "not every packet on this medium is ours" tolerance `discovery::tcp::recv_one` applies to UDP traffic it doesn't recognize).

Same gated-behind-`bluetooth`-feature, Linux-only, `bluer`-wrapped shape as `BluetoothChannel` above, and the same environment constraint applies: this container's kernel has no `AF_BLUETOOTH` support at all, so `advertise`/`scan` (both requiring a real adapter reachable via BlueZ's D-Bus API) could only be built to compile and reviewed by hand, not exercised. Per this task's own acceptance criteria, the parts of this module that don't need real hardware — the `encode_alias`/`decode_alias` pair, i.e. the entire advertisement payload's encode/decode round trip — are genuinely unit-tested (`cargo test -p flow-daemon --features bluetooth --lib discovery::bluetooth`); `advertise`/`scan` themselves are documented here as needing manual verification on a Linux machine with a real Bluetooth adapter and `bluetoothd` running, the same fallback G4 uses for its own hardware-dependent round trip.

### Channel negotiation (track G6)

`daemon/src/channel/negotiate.rs`'s `connect_best_available(addresses: &[ChannelAddress]) -> Result<Box<dyn Channel>, ChannelError>` is the one place in the daemon that knows both concrete `Channel` types exist — TCP is tried first (higher throughput, lower latency, which matters for a continuous mouse-move stream), Bluetooth only if no TCP address is present, and `ChannelError::Unreachable` if neither medium has an address at all. It takes `&[ChannelAddress]` rather than a single `DiscoveredPeer` — a deliberate, documented deviation from this task's originally-drafted signature: each discovery mechanism (`discovery::tcp`, `discovery::bluetooth`) produces a `DiscoveredPeer` with exactly *one* address (the medium that mechanism itself used), so "reachable via both mediums" can only be expressed once something merges a TCP discovery and a Bluetooth discovery of the same physical device into one list — reusing the existing `ChannelAddress` enum for that list rather than inventing a parallel struct. That merge is track G7's job.

Real hardware is only needed for the Bluetooth-only path's *success* case (an actual `bluer` connect against a real adapter — `#[ignore]`d for the same `AF_BLUETOOTH`-unsupported reason as `channel::bluetooth`'s own test); the TCP-preferred path, the both-mediums-present preference, the neither-reachable error, and the no-`bluetooth`-feature "unsupported medium" path are all genuinely exercised against a real loopback TCP socket (`cargo test -p flow-daemon --lib channel::negotiate`; `cargo test -p flow-daemon --features bluetooth --lib channel::negotiate` for the additional feature-gated cases).

### Real pairing over Channels (track G7)

`daemon/src/channel/handshake.rs` is the actual `PairingRequest`/`PairingDecision` exchange, carried as `ChannelMessage::Pairing` frames over whichever `Channel` `connect_best_available` negotiated — `request_pairing` (initiator) and `respond_to_pairing` (responder) both take `&mut dyn Channel` and never inspect `ChannelKind`, so this is the same code regardless of medium, proving `flow_core::channel::Channel`'s abstraction actually holds all the way up through pairing. `PairingRequest` gained a `device_os: HostOs` field in this task (previously just `device_name`/`address`) — needed to build a well-formed `Device` record on the responder side; safe to extend since nothing outside `core::pairing`'s and `core::channel`'s own tests constructed one, and it isn't part of the Flutter-facing contract (`docs/contracts/` never mentions `PairingRequest`).

`DaemonService` wires this into the existing mock-parity pairing state machine (track B4) rather than replacing it:

- **`note_discovered_peer(peer: DiscoveredPeer)`** registers a live-discovered peer (from `discovery::tcp`/`::bluetooth`, or — in this task's own integration test — injected directly) as a pairing candidate alongside where to reach it. Nothing in this codebase calls it except that test, so `B7`'s mock-parity suite, which never does, keeps exercising the pure `candidates_pool` mock flow completely unchanged — this *is* the "keep the hardcoded fallback behind a switch" acceptance criterion, just expressed as "whether any real peer was ever registered" rather than a separate config flag, since that was simpler and needed no new plumbing.
- **`pair_with_candidate`** checks whether the chosen candidate came from a live discovery. If so, it negotiates a real `Channel` (G6) and runs `channel::handshake::request_pairing` over it instead of the old fixed-delay timer; a mock `candidates_pool` candidate is unaffected. `PairingStage::Failed` (defined since track A3 but never actually reached by the mock flow) is now reachable for real: a rejected request or a `ChannelError` both land there with a message, auto-resetting to `Idle` after the same `PAIRING_TERMINAL_TO_IDLE` delay `Paired` uses (`on_paired_elapsed` was generalized into `on_terminal_elapsed(expected_stage)` to serve both, rather than duplicating the timer).
- **`accept_pairing_request(channel: Box<dyn Channel>)`** is the responder side: runs `channel::handshake::respond_to_pairing`, and on acceptance upserts the initiator into its own device list/`DeviceRepo`, so `docs/product/vision.md` §16's "once accepted, the devices become trusted" holds symmetrically on both ends of one handshake. **It always accepts** — the Flutter-facing contract has no incoming-pairing-request command or UI yet (`docs/contracts/daemon-ipc.md`'s `PairingSession` only models the initiating side's view), so a real accept/reject prompt on the receiving end would need a new contract command that doesn't exist; this is an honest scope boundary, not an oversight.

`daemon/tests/pairing_over_channel.rs` is the acceptance-criteria integration test: two independent `DaemonService` instances (each its own in-memory `Storage`) complete a real handshake over an actual loopback `TcpChannel` — one test for acceptance (both sides' devices list ends up containing the other), one for rejection (lands in `Failed`, not `Paired`, no device added). Neither uses the in-memory `ChannelPair` test double from `core::channel`'s own tests — genuine sockets throughout. This task's own scope was the handshake and state-machine integration, not the production listen loop — that landed later, in the integration pass "Daemon-to-daemon wiring in `main.rs`" documents below: `main.rs` now runs a real incoming-connection accept loop and feeds real TCP discovery into `note_discovered_peer` automatically. Bluetooth discovery (`discovery::bluetooth::scan`) is the one piece of this still not wired into anything, for the reason its own section above already gives (no `AF_BLUETOOTH` support in this container to build the wiring against).

Every third-party dependency that does real I/O (SQLite, WebSockets, Bluetooth, Noise encryption, OS input APIs) is wrapped behind a project-owned trait or type before anything else in the daemon depends on it — see `todos.json`'s `architecturalPrinciples.wrapThirdPartyDependencies` for the explicit rule and the module-by-module list of what wraps what.

## End-to-end input streaming pipeline (track G8)

`daemon/src/pipeline/mod.rs` is `vision.md`'s North Star made concrete — the first place capture, a `Channel`, and injection are wired into one continuous loop:

- **`send_while_active(capture_events, devices, channel, peer_id)`** — the sending side. Forwards every captured event onto `channel` as `ChannelMessage::Input`, but only while **`peer_id` is the `Active` device** per the `devices` watch stream; an event captured while this machine is the active one is silently dropped, not queued. The gate itself (`is_peer_receiving_input`) is a pure function over `&[Device]`, unit-tested directly without any async machinery. Gating on the specific peer, rather than just "the local device isn't active," is what stops a third paired device receiving a copy of everything.

  **This direction was inverted until the integration pass's review caught it.** `Active` means the device input is sent *to* (`vision.md` §22: "Only the active device should receive input"; the tray lists it under "Using"), so the machine holding the keyboard forwards only while *another* device is active. The original gate forwarded while the *local* device was active, which duplicated local typing onto the peer and stopped forwarding entirely the moment you switched away — and its unit tests encoded the same inverted expectation, so they passed throughout.
- **`receive_and_inject(channel, injector)`** — the receiving side. Injects every `ChannelMessage::Input` that arrives, ignoring anything else on the same connection (`Pairing`/`Heartbeat` traffic may share it).

Both are written entirely against `flow_core::channel::Channel` and `flow_core::input::InputInjector` — never a concrete medium or platform type. `platform::new_default_input_injector()` was added alongside this task: unlike `DefaultInputCapture`'s `new(sender)` (uniform across all three platforms), the injector's own constructors aren't — Linux/macOS are fallible (`new() -> Result<Self, _>`), Windows has no setup that can fail (`Default`) — so this one function absorbs that difference in `flow-platform` itself, keeping it "the only crate that needs to know which operating system it's running on."

Automated tests (`cargo test -p flow-daemon --lib pipeline`) exercise both functions against real, loopback-connected `TcpChannel`s and synthetic `InputEvent`s — deliberately not the in-memory `ChannelPair` test double `core::channel`'s own tests use (that type is private to `core`'s test module); a real loopback socket needs no more hardware or network access than an in-memory double would, so this was a reasonable, documented substitution rather than exporting a second test-only type across a crate boundary. The gating tests are worth reading for their synchronization discipline: rather than sleep-and-hope, the "streamed while Active" case reads the message straight off the peer's `Channel`, and the "dropped while Inactive" case proves the drop by closing the capture channel and observing the peer's `Channel` report `ConnectionLost` with nothing having arrived first — both deterministic, no arbitrary timing.

`daemon/examples/two_instance_streaming.rs` is this task's manual, real-hardware verification harness (the acceptance criterion's "manual two-instance-on-one-host test," `E1`-style): real capture -> the gate (always reporting `Active`, since one process stands in for two) -> a genuine loopback `TcpChannel` -> real injection. Same environment gap as `E1`/`E2`: this container has neither `/dev/input` nor `/dev/uinput`, so it's unverified beyond `cargo build -p flow-daemon --example two_instance_streaming`. Run manually on Linux with the right device permissions:

```sh
cargo run -p flow-daemon --example two_instance_streaming
```

This task's own scope was the pipeline functions and their gating logic, not full production wiring — that's `main.rs`'s job, covered next. `cargo check --target x86_64-apple-darwin`/`x86_64-pc-windows-msvc` for the whole `flow-daemon` crate (as opposed to just `flow-platform`, which E4-E7 already cross-compile-check) was not attempted: `rusqlite`'s bundled SQLite needs a real macOS/Windows C toolchain this container doesn't have, a pre-existing gap unrelated to this task's own changes — `flow-platform`'s own three-target check (which does cover `new_default_input_injector`) still passes.

## Daemon-to-daemon wiring in `main.rs`

Everything above this point (discovery, channel negotiation, the Noise handshake, the trust gate, and the streaming pipeline) was built and tested as standalone, well-isolated modules — real, but not yet the thing actually running when you started `flow-daemon` normally. A later integration pass spliced them together into `main.rs` itself, so two independently-started `flow-daemon` processes now find, pair with, and stream real input to each other without any test harness gluing the pieces together:

- **Peer channel listener.** `main.rs` binds a second TCP listener (`spawn_peer_channel_listener`) on an OS-assigned ephemeral port — distinct from the local IPC port (`127.0.0.1`-only) and from the discovery UDP port — bound to `0.0.0.0` since real peers reach it from elsewhere on the network. Its accept loop runs `DaemonService::accept_incoming_peer_channel` on every connection.
- **Discovery.** `spawn_discovery` owns one `DiscoveryService` (`discovery::tcp`, track G3) that both periodically re-announces this daemon's presence (every 5s) and listens for others', advertising the peer channel listener's actual bound port. Every received announce goes to `DaemonService::note_discovered_peer` (unchanged — the existing pairing-candidate flow) *and* to a redial attempt, described next. Each announce carries an `instance_id` (this daemon's own identity public key) so a broadcast to `255.255.255.255` echoing back to the sending host is recognized as ours rather than treated as a peer — without it every daemon "discovers" itself and handshakes against its own listener on every interval. A receive error logs and continues rather than ending the loop: a transient failure must not take discovery down for the rest of the process's life.
- **`DaemonService::accept_incoming_peer_channel(channel)`** (new) is the accept-side dispatcher: it runs the Noise handshake itself first — there's no peer identity to check against the trust store before it completes — then branches on the proven identity. An already-trusted peer gets handed back as a live `Channel` for `main.rs` to stream over; an untrusted peer is treated as a pairing attempt and handled in place, subject to the user's decision on the prompt (see "Incoming pairing consent" above), via a shared `accept_pairing_over` helper so this doesn't run the Noise handshake twice on one connection.
- **`DaemonService::dial_if_trusted(address)`** (new) is the redial-side counterpart: given a live-discovered peer's address, it negotiates a `Channel` and runs the Noise handshake as *initiator*, purely to find out who's actually there by proven identity — never by the self-reported name a discovery announce carries. Returns a live `Channel` only when that identity is already a paired device; otherwise it closes the connection and leaves `note_discovered_peer`'s ordinary pairing-candidate flow to handle it. The whole dial (connect plus handshake) is bounded by `DIAL_TIMEOUT` — a discovery announce is unauthenticated, so anything on the network can name an address that accepts TCP and then never completes the handshake. This is what makes a previously-paired device that becomes reachable again (Wi-Fi reconnect, the other daemon restarting) resume streaming with no user action — driven by discovery's 5s re-announce cadence, not `channel::reconnect::maintain_connection`'s own exponential-backoff loop, which remains real and tested but unused by this path (see "Auto-reconnect" below for why).
- **`pipeline::run_paired_connection`** (new, in `pipeline/mod.rs` alongside `send_while_active`/`receive_and_inject`) is what actually streams once a connection is established, in either direction: unlike the pairing handshake, an established peer connection needs to *both* send (while the peer is the active device) *and* receive-and-inject (while this machine is) over the *same* connection, since which side is active can flip. **The gate is on the *peer* being `Active`, not this machine** — `Active` means the device input is being sent *to* (`vision.md` §22, and the tray's "Using" heading), so a machine forwards only while some other device is active. It also drives local suppression (see "Local input suppression" above) on every switch. `Channel::send`/`::recv` both take `&mut self`, and nothing splits a `Channel` into independent read/write halves (`NoiseChannel`'s single shared `TransportState` would need real redesign to support that safely), so this single task interleaves both directions itself with `tokio::select!` rather than running the two existing one-way functions concurrently on two tasks sharing one channel. `main.rs`'s `run_peer_pipeline` gives it a dedicated real per-OS capture/injector pair for the connection's lifetime (the same construction `hotkey::runner::spawn` already uses for its own separate capture instance) and degrades gracefully, not fatally, if the platform adapter can't start.
- **Link health.** `run_peer_pipeline` calls the new `DaemonService::set_link_state` to push `DaemonLinkState::Connected` once a peer's pipeline actually starts, and — once the *last* live peer connection ends (checked against a shared set of connected peer ids, so one peer disconnecting doesn't downgrade link health while another is still live) — `Reconnecting`, matching `daemon-ipc.md`'s "connected -> (active link drops) -> reconnecting" transition, since discovery keeps re-announcing and will redial. `Disconnected` ("unreachable and not retrying") is reserved for `auto_reconnect` being switched off, the one case where nothing retries on its own. `ServiceState::from_storage` (the real, production loader) itself now starts fresh state at `Disconnected` too — a fresh daemon reports `Connected` only once a real peer connection has actually completed the full lifecycle (transport negotiation, Noise handshake, identity/trust verification, pipeline established), never as a seeded default. `ServiceState::seeded_for_test` keeps the old static `Connected` default, since that's the mock-parity behavior every test asserting it (and `MockDaemonRepository`) still expects.
- **Nothing long-running blocks the discovery loop.** `handle_discovered_peer` does only the cheap local `note_discovered_peer` call inline and spawns the dial-and-stream work onto its own task. Awaiting it in the `select!` loop would stop this daemon announcing and reading announces for the entire lifetime of a peer connection, so no third device could discover it.
- **Deduplication and simultaneous dials.** Both paths claim a peer's `DeviceId` in a shared `HashSet` (`try_claim_peer`) before running its pipeline. Claiming alone isn't enough, though: two paired daemons starting together dial each other at once, so each ends up with one connection it opened and one it accepted — and if each independently keeps "the one I opened," *both* connections get dropped by the other end and the pair can't talk. `DaemonService::connection_precedence` resolves that with a rule both sides compute identically: keep the connection opened by whichever side has the numerically smaller identity public key. Exactly one survives.

Verified: the full `cargo test --workspace` sweep (186 tests, including new coverage for the corrected forwarding direction, the suppression callback contract, the pairing consent window, and self-announce filtering), a clean `cargo clippy --workspace --all-targets -- -D warnings` (default and `--features bluetooth`) and `cargo fmt --check`, and manually running the built `flow-daemon` binary in this container — all three listeners (IPC, peer channel, discovery) come up and the hotkey runner degrades gracefully with no `/dev/input`. Not verified: two of these processes actually driving each other across genuinely separate physical machines, and none of the Linux `EVIOCGRAB` suppression path against real input devices — this container has neither a second machine nor `/dev/input`.

## Local input suppression

Forwarding captured input to a peer is only half of remote control. Capture is *passive* on all three platforms — an evdev read, a listen-only `CGEventTap`, a low-level hook that always chains onward — so a keystroke sent to the active device still reaches the sending machine's own applications too, and you end up typing into both computers at once. `vision.md` §22's "only the active device should receive input" requires the sending side to actively suppress its own.

`flow_core::input::InputCapture::set_suppress_local(bool)` is that half, called by `pipeline::run_paired_connection` whenever the active device changes (and always released when the connection ends, so a dropped link can never leave a keyboard captured):

| Platform | Suppression | How |
|---|---|---|
| Linux | **real** | Exclusive `EVIOCGRAB` (`evdev`'s `Device::grab`/`ungrab`) across every captured device, applied by the read loop on transition. Unverified against real hardware — this container has no `/dev/input` — beyond compiling and its own unit tests. |
| macOS | **not implemented** | Returns `MacosCaptureError::SuppressionUnsupported`. The tap is created `ListenOnly`, which by definition cannot swallow an event; suppression needs a *default* (active) tap whose callback returns null, with its own permission and `kCGEventTapDisabledByTimeout` re-arming behavior. |
| Windows | **not implemented** | Returns `WindowsCaptureError::SuppressionUnsupported`. A `WH_KEYBOARD_LL` hook *can* swallow an event by returning 1 instead of chaining to `CallNextHookEx` — no different API needed — but the hook procedures here read their state from thread-local storage, so the flag has to reach them through that same TLS. |

Both unimplemented cases deliberately return an **error** rather than `Ok(())`: a silent no-op would look like working suppression to the daemon, and the failure mode (every keystroke duplicated onto the local machine) is one the user should be told about. `main.rs` logs it once per transition and keeps streaming, since forwarding is still useful even when local input isn't suppressed.

Relatedly, Linux's device enumeration (`platform/src/linux/discovery.rs`) skips the injector's own `"Flow Virtual Input"` uinput node. Without that filter a second concurrent peer connection captures the first one's *injected* output and relays it onward, looping input between peers.

## Incoming pairing consent

`DaemonService::accept_pairing_request` / `accept_incoming_peer_channel`
surface every untrusted inbound pairing attempt to the connected UI as an
`IncomingPairingRequest` (`watch_incoming_request`, event
`incoming_pairing_request_changed`) and block the handshake until the
user answers via the `respond_to_pairing_request` IPC command. No UI
connected ⇒ the request is rejected immediately; no answer within
`PAIRING_DECISION_TIMEOUT` (30s) ⇒ rejected; only one request is prompted
at a time (others rejected while one is pending). This replaces the
earlier stage-based `is_pairing_window_open` stand-in.

This matters because the peer listener binds `0.0.0.0` and broadcasts its port every few seconds. Accepting unconditionally — which is what the responder did while it was unreachable in production, and which became live the moment `main.rs` grew a listener — would let any host on the local network add itself to the trust store with no user involvement, and then inject arbitrary keyboard and mouse input. Refusing by default is the only safe posture.

**What this is not:** the fingerprint shown in the prompt is display-only — there is no forced numeric comparison. A short-authentication-string (SAS) step, where both machines show the same code and the user confirms it matches, is deferred; the initiator side shows nothing to compare against yet, so a user accepting a request is trusting the device *name* and *address* in the prompt, not a verified key. Separately, there is no per-peer blocking or rate limiting beyond "one pending request at a time" plus the 30s auto-reject: a hostile LAN host can occupy the pending slot and re-arm it repeatedly, denying a genuine peer its turn for as long as it keeps trying. Both are deferred to a follow-up.

## Security posture during this phase

The local IPC channel (Flutter <-> this daemon) is bound to `127.0.0.1` only, and every connection to it must present a random token (`src/ipc/auth.rs`, persisted at `~/.flow/ipc.token`) as its WebSocket subprotocol before the handshake completes — `127.0.0.1` being reachable by any local process, not just the intended UI, is exactly the gap this closes; the daemon started up before this fix, but so could a connection from anything else on the machine. The daemon-to-daemon Channel (tracks G and H, both done) is the one that actually carries sensitive keyboard/mouse data, and is where real device identity, trust, and Noise encryption (`docs/product/vision.md` §17) apply — uniformly across both TCP and Bluetooth, since encryption wraps the `Channel` trait rather than either medium specifically.

### Pairing trust gate (track H2)

`daemon/src/trust/mod.rs`'s `TrustGate::is_trusted(public_key)` consults `P4`'s `DeviceRepo::is_trusted` to decide whether a peer claiming `public_key` is already a paired device — medium-agnostic by construction, since it never names a concrete `Channel` type, only a public key.

**This task's own scope was the gate function itself**, which `daemon/src/trust/mod.rs`'s own tests exercise directly against a real (in-memory) `DeviceRepo`: a public key upserted via `P4` is reported trusted, an unknown one isn't, and a never-paired database trusts nothing. An unauthenticated TCP/Bluetooth connection can claim any public key it likes, so checking `is_trusted` alone couldn't actually authenticate anything on its own — that needed cryptographic proof the peer holds the matching private key, which is `H3`'s Noise handshake. That wiring has since landed: `daemon/src/channel/gate.rs`'s `accept_trusted` (track H4, below) is where this gate actually gets consulted on a live connection, running `H3`'s handshake first to get a proven identity to check.

### Encrypted Channel decorator (track H3)

`daemon/src/channel/noise.rs`'s `NoiseChannel<C: Channel>` wraps any other `Channel` with an authenticated [Noise](http://www.noiseprotocol.org/) session (the `snow` crate — confined to this module) — `Channel::send`/`::recv` serialize/encrypt or decrypt/deserialize the whole `ChannelMessage`, so nothing above this layer ever sees plaintext go over the wire. `NoiseChannel::initiate`/`::accept` are the two sides of the `Noise_XX` handshake, run over the wrapped `Channel` as opaque `ChannelMessage::Noise(Vec<u8>)` frames (a new variant added for exactly this — never constructed or matched outside this module).

**Design note worth reading if you're extending this:** `H1`'s `DeviceIdentity` is an ed25519 *signing* key; Noise's `XX` pattern needs an X25519 *Diffie-Hellman* key — different primitives, and converting one into the other needs a birational-map conversion this codebase doesn't implement (hand-rolling that conversion for a security-critical path without a well-audited library felt like the wrong tradeoff). So `NoiseChannel` generates a fresh, ephemeral X25519 keypair per connection for the Noise handshake itself, and separately binds the resulting session to each side's `H1` identity by having both sides sign the handshake transcript (`get_handshake_hash()`, unique per session) and exchange `{public_key, signature}` as the first message over the newly-established encrypted transport. `NoiseChannel::peer_identity()` exposes the proven public key — what `H4`'s trust gate will consult. This is a standard, reasonable technique, but it hasn't been independently security-reviewed; treat it as this project's own best-effort implementation, not a substitute for a professional cryptographic audit.

`daemon/src/channel/noise.rs`'s own tests cover a full handshake (including that each side's `peer_identity()` matches the other's real `H1` public key) and a message round trip, against a real loopback `TcpChannel`. `daemon/tests/noise_channel.rs` is this task's acceptance-criteria integration test: a real byte-level packet sniff — an actual TCP relay sitting on the wire between two `TcpChannel` endpoints, recording every byte it forwards — proves a `ChannelMessage`'s plaintext (a distinctive marker string) never appears in the captured bytes when sent through a `NoiseChannel`. A second test is the sniff's own negative control: the identical marker sent over a *plain* `TcpChannel` (no Noise) *does* show up, proving the sniffing methodology would actually have caught a real leak rather than passing for an unrelated reason (worth noting: that control had to send server -> client rather than client -> server, since RFC 6455 requires every client-to-server WebSocket frame to be XOR-masked, which would have hidden the marker from a literal byte search regardless of encryption — a WebSocket-specific gotcha, not a `NoiseChannel` one). `grep -n "TcpChannel\|BluetoothChannel" daemon/src/channel/noise.rs` shows the only matches are in a doc comment and the `#[cfg(test)]` module — this task's second acceptance criterion (no concrete-medium reference in `NoiseChannel`'s own production code).

### Replay protection and unknown-device rejection (track H4)

Two independent checks, both living at the `Channel`/pipeline level so they apply the same way regardless of medium:

- **Connection-accept gate.** `daemon/src/channel/gate.rs`'s `accept_trusted(inner, local_identity, trust)` runs `NoiseChannel::accept` (`H3`) and then checks the resulting `peer_identity()` against `H2`'s `TrustGate` before returning a usable channel — an untrusted peer never gets a channel it could call `recv()` on, so no `InputEvent` from an unrecognized device can reach the pipeline. Worth noting: this task was originally drafted as "reject... before the Noise handshake completes," which isn't literally achievable — there's no peer identity to check against the trust store until the handshake's own identity-proof step produces one. What *is* achievable, and what this module delivers (matching the task's actual acceptance criterion), is rejecting an untrusted peer before any `InputEvent` can flow. Tested directly: a peer whose `H1` public key was upserted via `P4` is accepted; one that was never paired is rejected with `ChannelError::AuthenticationFailed`, before either side ever calls `send`/`recv` on the resulting channel.
- **Replay guard.** `pipeline::receive_and_inject` (`G8`) now tracks the last *accepted* event's `timestamp_ms` (`InputEvent::timestamp_ms()`, a new accessor reusing the field every event variant already carries, rather than adding a separate sequence number) and drops — doesn't inject — any incoming event whose timestamp isn't strictly greater. A replayed (duplicate timestamp) or out-of-order (stale timestamp) frame is silently skipped; the next genuinely newer event still gets through.

## Auto-reconnect (track I1)

`daemon/src/channel/reconnect.rs`'s `maintain_connection(addresses, settings, link_state, handle_connection)` keeps a connection to a peer alive across drops: it establishes a `Channel` via `G6`'s `connect_best_available`, hands it to `handle_connection` (the caller's own send/recv loop — `pipeline::send_while_active`/`::receive_and_inject`, or `daemon/tests/pairing_over_channel.rs`-style handshake logic, in production), and re-negotiates from scratch — not just retrying the same medium — every time that closure returns because the connection ended. Re-running the full negotiation on every attempt, rather than remembering which medium worked last, matches `docs/architecture/channels.md`'s own stated reasoning: the medium available when a device was first paired (e.g. home Wi-Fi) may not be the one available when the link drops and needs to recover (e.g. away from that network, Bluetooth-only).

This is what makes `DaemonLinkState` (`flow_core::link`) driven by real connection events end to end — `ServiceState::from_storage` starts fresh state at `Disconnected`, not a seeded `Connected` default. `maintain_connection` sets `Connecting` (first attempt) or `Reconnecting` (any attempt after a previous successful connection), `Connected` on success, and `Disconnected` — matching that variant's own "unreachable and not retrying" doc comment — the moment `settings.auto_reconnect` reads `false`, ending the loop rather than retrying forever. Backoff is capped exponential (100ms doubling to a 30s ceiling), reset to the initial delay on every successful connection.

Two tests, both against real loopback TCP sockets rather than a simulated failure: one has a real peer accept a connection, immediately close it, then accept and hold open a second one, and asserts the observed `DaemonLinkState` sequence is exactly `Connected -> Reconnecting -> Connected` with two distinct connections actually established; the other points `maintain_connection` at a port nothing is listening on with `auto_reconnect: false` and confirms it gives up on the first failure, landing on `Disconnected`, without an infinite retry loop.

**Still not called anywhere in `main.rs`, by a deliberate choice made in the later integration pass, not an oversight.** `main.rs`'s actual reconnect mechanism (see "Daemon-to-daemon wiring in `main.rs`" above) is discovery-driven instead: every ~5s re-announce that resolves to an already-trusted peer triggers a fresh `DaemonService::dial_if_trusted` attempt, which is what recovers a dropped connection once the peer is reachable again. That's a real, working reconnect path (`DaemonLinkState` does move `Connected -> Connecting` and back for real now — see `set_link_state` above), but it isn't `maintain_connection`'s own capped-exponential-backoff loop, and it doesn't honor `settings.auto_reconnect` the way `maintain_connection` does (discovery keeps re-announcing and redialing regardless of that setting). Splicing `maintain_connection` itself into the redial path — so a dropped connection retries on its own schedule instead of only when another discovery announce happens to arrive, and actually respects `auto_reconnect` — is a reasonable follow-up, not attempted in this pass to keep the redial logic in one place (`handle_discovered_peer`) rather than two overlapping retry mechanisms.

## Structured logging (track I2)

`daemon/src/logging.rs` wraps `tracing-subscriber`'s reload mechanism (confined to this module) so `settings.debug_logging` changes the daemon's actual log verbosity at runtime, matching `docs/product/vision.md` §15's Advanced settings "Debug logging" toggle — this is now wired to a real daemon rather than a no-op. `logging::init(debug_logging)` installs the process's one global subscriber (must run exactly once, as early as possible in `main`) and returns a `LoggingHandle`; `logging::spawn_debug_logging_toggle(service, logging)` syncs that handle to `service`'s real persisted setting immediately, then keeps it in sync on every subsequent `DaemonService::update_settings` call, for as long as the daemon runs. `main.rs` calls both at startup.

A sweep of `daemon`/`platform`/`core` confirms zero `println!`/`eprintln!` remain outside `daemon/examples/` (two small manual CLI harnesses — `linux_input_echo`, `two_instance_streaming` — which are standalone tools, not part of tracks B-H's own modules, and print user-facing status by design, the same way any CLI tool would); everywhere else already used `tracing::info!`/`debug!`/`warn!` before this task even started (`main.rs`'s connection-accept loop, `hotkey::runner`, `pipeline`). This task added `#[tracing::instrument(skip(self))]` to `DaemonService`'s own command methods (`switch_active_device`, `remove_device`, `start_pairing`, `cancel_pairing`, `pair_with_candidate`, `set_switch_key`, `update_settings`, `reset_settings`, `request_permission`) and to `ipc::server::handle_connection`, so every state-changing command and every IPC connection's lifecycle shows up as its own span in the logs — the "main service loops... connection handlers" this task's own deliverables named.

`daemon/src/logging.rs`'s own tests cover `LoggingHandle::set_debug`'s reload behavior directly (built over a real `reload::Layer` kept alive locally, not the process's actual global subscriber — `tracing_subscriber::registry().init()` can only run once per process, so a test can't install it without breaking every other test in the same binary) — plus, per this task's own acceptance criterion, one real end-to-end test: a genuine `DaemonService::update_settings(SettingsPatch { debug_logging: Some(true), .. })` call is shown to actually change `LoggingHandle::current_level()`, through the exact `spawn_debug_logging_toggle` wiring `main.rs` installs, not a simulated settings change. Manually confirmed too: running the compiled `flow-daemon` binary shows real `INFO`/`WARN` tracing output at startup (including the honest `hotkey runner not started: ... no keyboard- or mouse-capable /dev/input device found` warning this project's own development container always produces, per the "E1: Linux capture via evdev" manual verification note below).

## Manual verification notes

Several tasks in `todos.json` (E1-E7 below; G4/G5/G8 and I1 have their own equivalent notes inline in their own sections earlier in this file, not repeated here) can only be fully verified with real input devices, a second machine, or platform hardware this development environment doesn't have. Each such task's acceptance criteria says explicitly what was verified automatically (unit tests on pure translation logic, `cargo check` for cross-compiled platforms, integration tests against synthetic events) versus what still needs a human with the actual hardware to confirm — that discipline held for every task through the end of the plan (tracks A-J), not just this section's own E1-E7.

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

### E3: Linux capture/inject loopback harness

`daemon/examples/linux_input_echo.rs` wires E1's `LinuxInputCapture` straight into E2's `LinuxInputInjector`: every captured event is printed, then immediately replayed onto the virtual device, the minimum one-machine sanity check that exercises both adapters together. Needs the same `/dev/input`/`/dev/uinput` access as E1/E2, so it's manual/local-only — not part of `cargo test`.

```sh
cargo run -p flow-daemon --example linux_input_echo
```

Type or move the mouse (on a physical device the process can read); each event should print, and the same event should be observable on the new "Flow Virtual Input" device (e.g. via `evtest /dev/input/eventN`). Ctrl+C to stop — this repo's own container has neither `/dev/input` nor `/dev/uinput`, so only `cargo build --example linux_input_echo` (and, indirectly, E1/E2's unit tests) verify this here.

### E4: macOS capture via CGEventTap

`platform/src/macos/capture.rs` (`MacosInputCapture`) installs a `CGEventTap` (HID-level, listen-only) on a dedicated thread with its own `CFRunLoop`, translating each tapped `CGEvent` through the pure `EventTranslator` (`translate.rs`) and forwarding it over an `mpsc::Sender<InputEvent>` supplied at construction — the same shape as `LinuxInputCapture`. This container has no macOS hardware at all, so beyond `cargo check -p flow-platform --target x86_64-apple-darwin`/`clippy`/`fmt`, nothing here executed; `translate.rs`'s 12 unit tests construct synthetic `CGEvent`s via `CGEventSource` and exercise the translation logic in isolation, but — unlike E1/E2's Linux tests, which this container *can* run — they need an actual macOS process to execute at all, so they're written and cross-compile-checked only. On a Mac:

```sh
cargo test -p flow-platform --target <your-mac-target>   # runs translate.rs's unit tests for real
```

**Requires the Accessibility permission** (System Settings > Privacy & Security > Accessibility) for whatever process calls `MacosInputCapture::start()` — `CGEventTapCreate` fails silently (a null tap, not a loud error) without it, surfaced here as `MacosCaptureError::TapCreationFailed`. What to look for on real hardware: `start()` returns `Ok(())` only once that permission is granted; typing/clicking/scrolling produces the expected `InputEvent`s on the channel, including the correct `modifiers` list for chorded keys; and `stop()` returns promptly (`CFRunLoop::stop()` unblocks `CFRunLoop::run_current()` on the capture thread almost immediately, unlike E1's idle-poll delay).

### E5: macOS injection via CGEventPost

`platform/src/macos/injector.rs` (`MacosInputInjector`) posts synthetic `CGEvent`s built from incoming `InputEvent`s via `CGEventPost`, through the pure(-ish) `inject_translate::to_cg_event` function (the reverse of E4's `EventTranslator`; "pure-ish" since building a `CGEvent` is a real Core Graphics call, not just struct construction, but it needs no tap or permission). One notable design choice: `MouseEvent::Move` carries a relative delta, but `CGEvent::new_mouse_event` wants an absolute position — this daemon doesn't track the cursor's actual location, so the posted event is anchored at wherever the cursor currently is (read via a throwaway `CGEvent::new(source).location()`) with the delta layered on top via the `MOUSE_EVENT_DELTA_X`/`Y` fields, which `CGEventPost` honors for relative motion. Verified with `cargo check -p flow-platform --target x86_64-apple-darwin` and `--target aarch64-apple-darwin` (both `cargo check` and `clippy --all-targets`, per E5's acceptance criteria specifying `aarch64-apple-darwin`), plus `cargo fmt` — no macOS hardware exists here, so `inject_translate.rs`'s 7 unit tests (constructing `InputEvent`s and asserting on the resulting `CGEvent`'s type/fields, the same style as E4's tests) have never actually executed:

```sh
cargo test -p flow-platform --target <your-mac-target>   # runs inject_translate.rs's unit tests for real
```

What to look for on real hardware: injected keypresses/clicks/scrolls/moves are indistinguishable from real input to other applications (the whole point of `CGEventPost`); a posted `MouseEvent::Move` moves the cursor by the given delta from wherever it already was, not to a fixed point; and the E3-style loopback idea (capture -> inject on one machine) would need care to avoid feedback loops, since posted events re-enter the same HID event stream a listen-only tap also observes — unlike Linux, where E3's virtual device is a distinct kernel input node the read loop never taps.

### E6: Windows capture via SetWindowsHookEx

`platform/src/windows/capture.rs` (`WindowsInputCapture`) installs `WH_KEYBOARD_LL` and `WH_MOUSE_LL` hooks on a dedicated thread and pumps that thread's message queue (`GetMessageW`/`DispatchMessageW`), the OS's own requirement for low-level hooks — the callback runs on whichever thread called `SetWindowsHookExW`. Hook procedures are plain `extern "system"` function pointers with no user-data slot (unlike `CGEventTapCreate`'s closure-based callback), so the translator and output channel live in thread-local storage instead, populated before the hooks go up and cleared after the message loop exits; `stop()` posts `WM_QUIT` to that specific thread via `PostThreadMessageW` to unblock it. Translation (`translate.rs`) has one wrinkle the other two platforms don't: the low-level mouse hook reports an *absolute* cursor position (`MSLLHOOKSTRUCT.pt`), not a delta, so `EventTranslator` tracks the last reported position itself and diffs consecutive moves — the first move after `start()` has nothing to diff against and is dropped. No Windows hardware exists in this environment, so beyond `cargo check`/`clippy --all-targets -D warnings`/`cargo fmt` for `flow-platform` on `x86_64-pc-windows-msvc`, nothing here executed; `translate.rs`'s unit tests construct synthetic `KBDLLHOOKSTRUCT`/`MSLLHOOKSTRUCT` values directly (no hook needed) but, like E4/E5's tests, need Windows to actually run:

```sh
cargo test -p flow-platform --target <your-windows-target>   # runs translate.rs's unit tests for real
```

What to look for on real hardware: `start()` succeeds without any special permission (unlike macOS's Accessibility gate, low-level hooks need no user consent, though some antivirus/EDR software flags them); typing/clicking/scrolling produces the expected `InputEvent`s, including per-side modifier names (`LSHIFT` vs `RSHIFT`) and a normalized one-unit-per-notch `Scroll`; and `stop()` returns once the posted `WM_QUIT` is processed — near-instant, similar to macOS's `CFRunLoop::stop()` and unlike Linux's idle-poll delay.

### E7: Windows injection via SendInput

`platform/src/windows/injector.rs` (`WindowsInputInjector`) builds `INPUT` structs from incoming `InputEvent`s through the pure `inject_translate::to_input` function (the reverse of E6's `EventTranslator`) and queues them via `SendInput`. Unlike macOS's `CGEventPost` (one event per call, needing a manual anchor-to-current-position hack for relative moves) `SendInput` takes `dx`/`dy` as a genuinely relative delta when `MOUSEEVENTF_ABSOLUTE` isn't set, so `MouseEvent::Move` translates directly with no cursor-tracking workaround needed. One design choice worth noting: a `MouseEvent::Scroll` with both axes set becomes *two* `INPUT` entries in one `SendInput` call (`MOUSEEVENTF_WHEEL` and `MOUSEEVENTF_HWHEEL` are mutually exclusive on a single `INPUT`), the same per-axis shape E2's Linux uinput injector uses — `to_input` returns `Vec<INPUT>` rather than a single value for exactly this reason. Verified with `cargo check`/`clippy --all-targets -D warnings`/`cargo fmt` for `flow-platform` on `x86_64-pc-windows-msvc`, all clean — no Windows hardware exists here, so `inject_translate.rs`'s 10 unit tests (constructing `InputEvent`s and reading back the resulting `INPUT`'s union fields, unsafely but only ever reading back what the same test just wrote) have never actually executed:

```sh
cargo test -p flow-platform --target <your-windows-target>   # runs inject_translate.rs's unit tests for real
```

What to look for on real hardware: injected input is indistinguishable from real hardware input to other applications, the same as macOS's `CGEventPost`; `SendInput` returns fewer queued events than sent when something (commonly a UIPI-elevated foreground window) is blocking synthetic input — `WindowsInjectError::SendInputBlocked` surfaces that rather than silently dropping it; and a `MouseEvent::Move` moves the cursor by the given delta regardless of where it already was, unlike macOS's anchor-and-offset approach.

With E7 landed, all three platforms (Linux, macOS, Windows) have both capture and injection implemented — Linux's E1-E3 are the only ones actually exercised end-to-end in this environment; macOS's and Windows' are cross-compile-checked and unit-tested-in-source only, per each section above.
