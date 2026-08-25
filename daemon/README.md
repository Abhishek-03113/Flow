# flow-daemon

The Rust data-plane daemon for Flow — input capture, injection, device switching, networking, and pairing, running independently of the Flutter UI (`docs/product/vision.md` §8). This directory holds the daemon binary itself; the workspace also includes `flow-core` (`../core`) and `flow-platform` (`../platform`).

**Status: real process, real OS input, no networking yet.** `flow-daemon` binds a WebSocket IPC listener on `127.0.0.1:47823`, serves the full `docs/contracts/daemon-ipc.md` contract (backed by `flow-core`'s mock-parity `DaemonService`) over it, and persists to a local SQLite database — tracks A/P/B/C are done. `flow-platform` has real capture/injection for Linux (evdev/uinput, exercised end-to-end in a dev container), macOS (`CGEventTap`/`CGEventPost`), and Windows (`SetWindowsHookEx`/`SendInput`) — tracks E1-E7 done, though only Linux's has run beyond compiling in this development environment (see "Platform adapters" below). `SwitchKeyMatcher` (track F1) detects the configured switch-key combination over that capture stream; wiring it to actually trigger a device switch (F2) and daemon-to-daemon networking (track G) haven't landed yet. See [`todos.json`](./todos.json) and [`PLAN.md`](./PLAN.md) for the full build-out plan; a full section-by-section pass (not just this status line) is track J5's job once everything above lands.

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
| macOS | real (E4, `CGEventTap`) | real (E5, `CGEventPost`) | cross-compile checked (`cargo check -p flow-platform --target x86_64-apple-darwin` and `--target aarch64-apple-darwin`) plus `clippy`/`fmt` on both; the `CGEvent <-> InputEvent` translation has unit tests in both directions, but they need macOS to execute — this Linux container can only compile-check them, not run them, and there's no Mac hardware here at all — see "Manual verification notes" below |
| Windows | real (E6, `WH_KEYBOARD_LL`/`WH_MOUSE_LL`) | real (E7, `SendInput`) | cross-compile checked (`cargo check`/`clippy --all-targets` for `flow-platform` on `x86_64-pc-windows-msvc`) plus `fmt`; the `InputEvent <-> INPUT` translation has unit tests in both directions, but no Windows hardware exists here to run them — see "Manual verification notes" below |

Cross-compilation setup instructions land in `todos.json` task J3 once the macOS/Windows adapters exist to check.

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

Settings, paired devices (which double as the trust store), this daemon's own identity keypair, and a connection history log all live in a single local SQLite database (`rusqlite`, bundled — no system SQLite dependency) under the platform data directory, applied via versioned migrations on startup. Nothing here is derived fresh on every run or held only in memory: a fresh database bootstraps to the same seed data the mock uses (3 devices, defaults), and every subsequent run loads what was actually persisted. `daemon/todos.json` track **P** (`persistence-storage`) builds this, positioned right after the core contract types and ahead of the command service itself, since `DaemonService`'s startup state depends on it.

## Process supervision (not yet implemented)

The daemon is meant to run independent of any UI, ideally started at login/boot and restarted automatically if it crashes (`docs/product/vision.md` principle 7). This phase scaffolds but does not install service unit files (`todos.json` J4: a `launchd` plist, a `systemd` unit, notes for a Windows service wrapper) — actually registering the daemon to auto-start is future work beyond this plan.

## Channels (daemon-to-daemon connectivity)

Daemon-to-daemon connections — the one that actually carries keyboard/mouse input between two machines — go through a single custom abstraction named **Channels**: a `Channel` trait (`flow_core::channel`, track G1) implemented by `TcpChannel` (`daemon/src/channel/tcp.rs`, track G2 — Wi-Fi/local network, via a wrapped WebSocket) and `BluetoothChannel` (`daemon/src/channel/bluetooth.rs`, track G4 — RFCOMM/Bluetooth Classic, via a wrapped `bluer`), with a negotiation step that prefers TCP and falls back to Bluetooth when no shared network exists (G6, not yet built). Full design, wire shape, and explicit scope boundaries: [`docs/architecture/channels.md`](../docs/architecture/channels.md). This is a separate document from `docs/contracts/` on purpose — Channels is daemon<->daemon, the contracts directory is Flutter<->daemon — kept in the same documentation style so the two are easy to reconcile if Channel state ever needs to surface into the UI later. `daemon/todos.json` track **G** builds this.

`daemon/src/discovery/tcp.rs` (track G3) is how a peer's `TcpChannel` address gets found in the first place: a UDP broadcast announce/listen loop (`DiscoveryService`, port `47824`) advertising `{name, os, channel_port}`, producing `DiscoveredPeer { name, os, address: ChannelAddress::Tcp(..) }` values. Real broadcast (`255.255.255.255:47824`) is what `DiscoveryService::broadcast_destination()` targets in normal use; this session's own sandboxed network rewrote broadcast packets unpredictably when tested directly (a real send to `255.255.255.255` came back with its source address rewritten to a `192.0.2.x` TEST-NET address), so this module's own tests exercise the identical send/receive/parse code path via directed loopback (`127.0.0.1:<peer's bound port>`) instead of relying on genuine broadcast fan-out, which containerized/sandboxed networks often don't support reliably anyway. Bluetooth peer discovery (advertising/scanning for `ChannelAddress::Bluetooth` peers, the Bluetooth counterpart to this module) is track G5 and has not landed yet.

Nothing yet calls `TcpChannel::connect`/`::accept`, `BluetoothChannel::connect`/`::accept`, or `DiscoveryService::spawn_listener` outside their own tests — no pairing handshake is wired to any of them (that's track G7) — so two daemons can't actually find and pair with each other over the network today, but the TCP transport, the discovery mechanism, and the Bluetooth transport are all real, tested against real sockets to the extent this environment allows (`cargo test -p flow-daemon --lib channel::tcp discovery::tcp`; `cargo test -p flow-daemon --features bluetooth --lib channel::bluetooth`).

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
