# Manual Testing Strategy (single physical device)

**Constraint this document is written for:** one physical machine, no second computer available yet. Flow's entire premise is controlling a second computer, so this is the constraint that matters most for manual (human-driven) verification — automated tests don't care how many machines exist, but a human confirming "did it actually feel right" eventually does.

**Approach:** don't wait for a second machine to start manual testing. Most of the stack is checkable today, on one machine, because of how the architecture is layered — the local IPC contract (`docs/contracts/`) never needed two machines to begin with, and even the daemon-to-daemon protocol (`docs/architecture/channels.md`) can be exercised correctly with two processes on one host before it's ever exercised across two real desktops. What genuinely needs something beyond one bare-metal OS is the *felt* experience — watching a keystroke happen on a different screen — and that's solved with a VM, not a purchase.

This document is organized as tiers, ordered by what's actually testable **today** given `daemon/todos.json`'s real progress, through what becomes testable as tracks E/F/G/H land, through what still needs something beyond this one machine.

## Where the daemon actually stands right now

Tracks **A, P, B, C, D** are done: `flow-core`'s contract types, SQLite persistence, `DaemonService`, the local IPC WebSocket server, and Flutter's `IpcDaemonRepository` all exist and pass their test suites (`daemon/todos.json`). Concretely, **a real `flow-daemon` process and the real Flutter UI can already talk to each other today**, on this one machine — this isn't cross-device yet (the daemon's device list is still mock-parity seed data, not real network discovery), but the entire IPC contract, the SQLite-backed persistence, and every screen's provider wiring are real, not mocked. That's Tier 0 below, and it's available *right now*, not after tracks E-J land.

Tracks **E** (platform input capture/injection), **F** (switch-hotkey), **G** (Channels — TCP/Bluetooth networking), and **H** (security) are not started. Those are what turn this into an actual keyboard/mouse-sharing product, and they're also where the one-device constraint starts to bite — captured in Tiers 2-4 below.

## Tier 0 — Flutter ↔ real daemon, one machine (testable today)

Nothing here needs a second device; it never did. This is the local IPC contract (`docs/contracts/`), which was always scoped to "this machine's UI talking to this machine's daemon."

```sh
# terminal 1, repo root
cargo run -p flow-daemon

# terminal 2
cd flutter && flutter run -d linux --dart-define=FLOW_DAEMON_MODE=ipc
```

This swaps `daemonRepositoryProvider` from `MockDaemonRepository` to `IpcDaemonRepository`, connecting to the real daemon on `ws://127.0.0.1:47823`. Manual checklist:

- [ ] Tray popover shows the daemon's real seed data (3 devices) instead of the mock's — confirms you're actually on the IPC path, not silently still on the mock.
- [ ] Switch active device from the UI; kill and restart `flow-daemon`; confirm the previously-active device is still active (SQLite persistence, not memory).
- [ ] Change a setting (switch key, pointer sensitivity, a toggle); restart the daemon; confirm it stuck.
- [ ] Start pairing from the UI, accept the mock candidate, confirm the daemon's seeded pairing timings (`sharedContractConstants.mockParityTimings` in `daemon/todos.json`) show up as real delays, not instant.
- [ ] Kill `flow-daemon` mid-session; confirm the UI shows the daemon as unreachable rather than hanging or crashing.
- [ ] Run the existing automated cross-language contract test (`daemon/README.md` "Cross-language contract test") — this already exercises the 13 mock-parity scenarios against the real daemon; run it once per work session as a fast regression check before doing anything manual.

This tier stays valid and worth re-running after every track E-J change, since a regression here (the IPC contract breaking) would be caught immediately and cheaply, before chasing it through capture/networking code.

## Tier 1 — Real input capture/injection, one machine, loopback (needs track E)

`daemon/todos.json` E3 is exactly this: capture real keyboard/mouse locally, inject into a virtual device, observe the result — all on one machine, no network involved.

**Safer than it sounds:** don't inject straight back into your live desktop session at first — a captured-then-immediately-reinjected real keyboard can cause feedback loops or just be confusing to watch. Recommended sequence:

1. Run the capture side alone; log decoded `InputEvent`s to stdout. Confirms E1 without touching injection at all.
2. Create the virtual `uinput` device (E2) and watch it with `evtest /dev/input/eventN` in a separate terminal — confirms injection produces the right key codes/mouse deltas without ever touching your real desktop's input focus.
3. Only once both are independently confirmed, wire them together per E3's loopback example, and consider pointing the injected output at a nested display (e.g. `Xephyr`) rather than your main session, so a bug in the loopback can't take over your real desktop mid-test.

## Tier 2 — Real switch-hotkey, one machine (needs track E + F)

This is fully testable on one machine, no second device needed at all: press your configured switch key, watch `DaemonService`'s active device flip (visible live in the Tier-0 Flutter UI, since it's reading the same `devices_changed` stream). This is actually a good one to test early relative to networking — F doesn't depend on G, so hotkey-triggered local state transitions can be verified well before any Channel code exists.

- [ ] Configure a non-default switch key from the UI (Settings → Input); confirm the daemon picks it up without a restart (F1's acceptance criteria).
- [ ] Rapid repeated presses (holding Scroll Lock, or a noisy combo release) produce exactly one switch, not several (F3 debounce).
- [ ] With no UI running at all, press the switch key, then launch the UI — confirm the state it shows matches what actually happened (proves F2's "works without a UI attached" requirement, not just "works while being watched").

## Tier 3 — Full Channel protocol, one machine, two processes (needs track G)

This is the tier most people assume needs two computers. It doesn't — for *protocol correctness* — because TCP loopback and TCP-over-LAN are the same code path, and `daemon/todos.json`'s own G-track acceptance criteria are already written this way ("two daemon instances on the same host... discover each other via loopback broadcast," "two local daemon instances complete a real pair_with_candidate handshake"). Two `flow-daemon` processes on one machine, each with its own data directory and ports, validate discovery, negotiation, pairing, encryption, and message exchange for real.

**Prerequisite to check when G lands:** confirm the discovery/Channel ports are overridable per-instance (env var or CLI flag) so two processes don't collide — `daemon/todos.json`'s own G3 task already assumes "bound to different ports for the test," so this should already be part of the implementation, not something to bolt on. The fixed IPC port (`47823`, Flutter-facing) is a separate concern: you don't need two of those for this tier, since only one side needs a visible UI (see below).

Practical setup for a *manual* (not just automated) run:

```sh
# "device A" — the one you'll watch through the real Flutter UI
FLOW_DATA_DIR=/tmp/flow-a cargo run -p flow-daemon  # exact flag/env name: confirm against G's actual CLI once implemented

# "device B" — driven headlessly, no Flutter needed on this side
FLOW_DATA_DIR=/tmp/flow-b FLOW_CHANNEL_PORT=<other-port> cargo run -p flow-daemon
```

Drive device B's pairing acceptance either via a second `flutter run --dart-define=FLOW_DAEMON_MODE=ipc` pointed at its own IPC port (if that becomes configurable) or, more simply, with a raw WebSocket client (`websocat ws://127.0.0.1:<B's IPC port>`, or a short Python/Node script) sending the same JSON envelope `docs/contracts/daemon-ipc.md` documents — no second UI build needed just to click "Accept" on the other side.

- [ ] Device A discovers device B (and vice versa) over loopback.
- [ ] Pairing handshake completes; both sides persist the other (restart both, confirm the pairing survived).
- [ ] Kill device B mid-session; confirm device A's link state moves through the `daemon-ipc.md` state machine (`reconnecting` → `disconnected` or recovery) rather than hanging.
- [ ] Attempt a connection from a third, never-paired daemon instance; confirm H's trust gate rejects it (once H lands).
- [ ] Sniff the raw loopback traffic (`tcpdump -i lo -A 'tcp port <channel port>'`) once `H3`'s `NoiseChannel` lands; confirm you cannot read a plaintext `InputEvent` in it.

This tier gives real confidence in everything except the one thing it structurally can't: what it feels like to watch a keystroke land on a *different screen*. That's Tier 4.

## Tier 4 — The felt experience, one physical machine (needs a VM, not a purchase)

Two `flow-daemon` processes on one OS still write to the same desktop session — there's no second screen to watch input "arrive" on. A local VM solves this without needing a second physical machine:

1. Install a free hypervisor (VirtualBox, QEMU/KVM, or GNOME Boxes) and create one lightweight Linux VM. Bridged or host-only networking so the VM gets its own IP — mDNS/UDP discovery (G3) will treat it as a genuinely separate host, not localhost, closing a real gap loopback testing can't close (multicast behavior across an actual interface, not just `lo`).
2. Cross-compile or build `flow-daemon` for Linux inside the VM (same OS family as the host, so no cross-compilation needed at all if the host is also Linux).
3. Run `flow-daemon` inside the VM. It has its own display and its own virtual input focus, so E2's injected keystrokes now visibly land in the VM's window — you can watch a keystroke made on your real, physical keyboard (captured by the host's `flow-daemon`) appear inside the VM's screen. This is the actual product experience, achieved with one physical machine.
4. This is also the first point at which Bluetooth is worth mentioning as a gap: most hypervisors don't virtualize a Bluetooth radio usefully, so `BluetoothChannel` (G4/G5) realistically can't be manually verified this way. It stays untested until either a second physical Bluetooth-capable device exists or the host and VM can be given real, separate Bluetooth adapters (e.g. two USB Bluetooth dongles passed through to two VMs) — a fallback worth knowing about, not a recommended default.

**A cheap path to a *real* second device, when ready:** a Raspberry Pi (~$35-50) running Linux is a genuine second machine, not a simulation — worth it once G/H are stable enough to be worth testing against real hardware instead of a VM, and cheaper than it sounds relative to what it unblocks.

## Tier 5 — Cross-platform gap (macOS/Windows)

`daemon/todos.json` E4-E7 are written and cross-compile-*checked* only — no macOS or Windows hardware exists in the development environment this plan was built in, and a VM doesn't fix that (macOS VMs are impractical without Apple hardware to license/run them on; a Windows VM is workable but still isn't "real Windows input APIs on real hardware" for every edge case). Realistic options, cheapest first:

1. **Borrow a machine for an afternoon** (a friend's Windows laptop, a work-issued Mac) and run the Tier 5 checklist below once. Even a single manual pass beats zero.
2. **A cloud VM** for the Windows side specifically (Azure/AWS Windows instance) — cheap for a short burst, good enough to confirm `SetWindowsHookEx`/`SendInput` behave as E6/E7 assume.
3. **A cloud macOS instance** (MacStadium, AWS EC2 Mac) — pricier and less casual, but exists if a Mac genuinely can't be borrowed.
4. Until one of the above happens: macOS/Windows capture/injection stay explicitly "written, not verified" — say so rather than implying parity with Linux, matching the honesty standard `daemon/todos.json` already holds E4-E7 to.

## Tier 6 — Full real two-device checklist (run once real second hardware — VM, Pi, or borrowed machine — is in place)

The actual product acceptance checklist, distinct from anything above because it's about *feel*, not correctness:

- [ ] Pairing UX end-to-end, from a cold start on both sides, following only the on-screen instructions (no prior knowledge of the flow).
- [ ] Switch-key latency — does it feel instant, or is there a perceptible lag (vision.md's "instant interaction" principle)?
- [ ] Mouse movement/sensitivity across two screens of different resolution/DPI.
- [ ] Disconnect the network mid-session (turn off Wi-Fi); confirm reconnect (I1) recovers without restarting anything.
- [ ] With both TCP and Bluetooth available, confirm G6 actually prefers TCP; disable Wi-Fi only, confirm it falls back to Bluetooth.
- [ ] Restart both daemons; confirm the pairing and settings on both sides survived (P3/P4).
- [ ] Attempt to pair a third, unrelated device mid-session; confirm it doesn't disrupt the existing active pair.

## Summary: what to actually do, in order

1. **Now:** run Tier 0. It's real, it's available today, and it's the cheapest regression check to keep re-running as E-J land.
2. **As F lands:** Tier 2, still zero extra hardware.
3. **As E lands:** Tier 1, with the safety sequencing above.
4. **As G lands:** Tier 3 first (two processes, one machine — protocol correctness), then set up the Tier 4 VM once G/H feel stable enough to be worth watching visually.
5. **Before claiming Windows/macOS support works:** Tier 5 on borrowed or cloud hardware — don't skip this by assuming Linux parity.
6. **Once real second hardware exists in any form:** Tier 6, as the final human acceptance pass before considering a milestone actually done.
