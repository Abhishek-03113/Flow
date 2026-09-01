# Flow V1 — User Journeys

Derived from [`docs/product/vision.md`](../product/vision.md) and [`../../README.md`](../../README.md).
This is the **product-first** journey inventory: the smallest version of the Flow vision that
has to actually work on two physical computers.

> **V1 in one sentence:** one physical keyboard + one physical mouse control two physical
> computers (Windows and Mac), and pressing **Scroll Lock** switches which computer receives
> the input. Exactly one active destination at any moment.

## Scope

**In scope for V1** — everything needed for the sentence above: daemon + UI startup,
discovery, pairing, physical keyboard/mouse capture, forwarding to the one active peer,
local input suppression on the sending machine, remote injection, Scroll Lock switching in
both directions, disconnect/reconnect, restart persistence, and a UI that reflects the real
master/slave state.

**Out of scope for V1** (from `vision.md` §23 "Future Possibilities" and the product-first
brief) — multi-peer routing, event broadcasting, >1 simultaneous destination, routing
graphs, Bluetooth transport, Virtual HID, protocol redesign, clipboard/file transfer,
mouse-position-based switching, >2 computers. None of these gate V1 and none should be
touched to make V1 work.

## Status legend

- `[x]` works — implemented and verified (unit tests + at least one real run of the path)
- `[~]` partial — implemented and unit-tested, but **not** verified end-to-end on two
  physical machines, or works in one direction/platform only
- `[ ]` broken — a real code gap; the journey cannot currently succeed
- `[-]` intentionally out of scope for V1

Every `[~]` in this document shares one root reason: the codebase is
"implemented + unit-tested" but **has never been run across two real machines**
(`daemon/README.md` is explicit about this). Physical validation is the single largest
outstanding item and only the maintainer can perform it — see
[`manual-testing-strategy.md`](manual-testing-strategy.md) Tier 5/6.

---

## Journey 1 — Start the daemon `[x]`

```
run flow-daemon
  ↓
opens SQLite state (settings, devices, identity, history)
  ↓
binds: IPC 127.0.0.1:47823 · peer channel 0.0.0.0:<ephemeral> · discovery UDP 0.0.0.0:47824
  ↓
starts hotkey runner (switch key works with no UI)
  ↓
local device is available; daemon serves the IPC contract until Ctrl-C
```

Verified: builds and runs on this Windows host; `todos-get-app-working.json` track A/C.
Startup misconfig (port in use, unwritable data dir) exits non-zero with a one-line reason,
not a panic.

## Journey 2 — Start the UI `[x]`

```
flutter run (FLOW_DAEMON_MODE=ipc)
  ↓
connects to the daemon over local IPC (token-authenticated)
  ↓
first launch → onboarding (welcome → permission → pair → done)
  ↓
dashboard shows the real local device and real link state
```

Verified live on Windows against a real daemon (`todos-get-app-working.json` tracks B, D).
No mock/placeholder data in the shipped UI.

## Journey 3 — Daemon works without the UI `[x]`

```
UI closed / never started
  ↓
capture, forwarding, injection, and Scroll Lock switching all still run
  ↓
UI can attach later and shows the true current state
```

`vision.md` §8 principle. The hotkey runner and the peer pipeline are independent of any IPC
client. Verified for the switch key path; the forwarding path inherits the same
independence.

## Journey 4 — Discover the second device `[x]`

```
Device A daemon        Device B daemon
      │  UDP announce to subnet broadcast every 5s  │
      └───────────────────┬────────────────────────┘
                          ▼
        each parses the other's announce → pairing candidate
```

Verified with two daemon instances on one Windows box (`todos-get-app-working.json` track A).
A real Mac running `flow-daemon` was also observed as a live LAN candidate during that
work. Multi-homed-host and firewall caveats are documented in `daemon/README.md` ("Channels").

## Journey 5 — Pair the two devices `[~]`

```
select the discovered device → Pair
  ↓
negotiate a channel (TCP) → Noise handshake → identity proof
  ↓
consent on the receiving side (untrusted peer ⇒ IncomingPairingRequest prompt)
  ↓
both daemons persist the other in the trust store; link state → Connected
```

`[x]` between two Windows instances (mutual persistence + both link states Connected,
verified). `[~]` **Windows ↔ Mac specifically** has not been completed end-to-end — it needs
the maintainer to press "Pair" on the Mac too (`todos-get-app-working.json` D7).

## Journey 6 — Use the keyboard on the remote device `[~]`

```
Windows is master (Mac is the Active destination)
  ↓
physical key press on Windows → captured (WH_KEYBOARD_LL)
  ↓
forwarded to Mac only (gated on the peer being Active) + suppressed locally
  ↓
Mac injects the keystroke (CGEventPost); Windows app does NOT see it
```

`[~]` Forwarding + sequence/replay handling + held-key release: implemented, unit-tested,
and exercised over real sockets by `drive_two_daemons`. Windows local suppression
(`SuppressionGate`, return `LRESULT(1)`) + self-injection guard (`dwExtraInfo` marker, added
iteration 3): implemented and unit-tested, but never run on real hardware. Never
demonstrated Windows → Mac on two machines.

## Journey 7 — Use the mouse on the remote device `[~]`

```
move / left-right-middle click / scroll on Windows
  ↓
same capture → gate → forward → suppress → inject path as Journey 6
  ↓
Mac cursor moves / clicks / scrolls; Windows cursor unaffected
```

`[~]` Same status as Journey 6. Note: a recent fix (`7673651`) corrected macOS injected
mouse-move (cursor never moved) and left-click (missed) — relevant for the reverse
direction and unvalidated on hardware.

## Journey 8 — Switch control (Scroll Lock) `[~]`

```
press Scroll Lock (Windows master)
  ↓
consumed locally — not delivered to the local app, not forwarded to Mac
  ↓
active device flips: Mac → slave, Windows → master
  ↓
next physical event routes to the new destination
```

`[~]` `SwitchKeyMatcher` + 500 ms `SwitchDebouncer` + live rebind + pipeline-level switch
filter (`spawn_pipeline_switch_filter`, strips the switch key's KeyDown **and** its matching
KeyUp from the forwarded stream): implemented, unit-tested. The hook-ordering hazard (two
LL hooks; a suppressing hook ends the chain before the hotkey runner's) is resolved by
"Option A" — the peer pipeline owns switch authority while it runs — merged, not yet proven
live. `[ ]` for a multi-key binding, modifier down/up still leak to the peer; the default
Scroll Lock binding is clean.

## Journey 9 — Switch back (Scroll Lock) `[~]`

```
press Scroll Lock again (Mac master)
  ↓
Mac consumes it locally, does not forward it
  ↓
Windows → master, Mac → slave
```

`[~]` Symmetric with Journey 8. **Blocked on Journey R (below):** for the Mac to be a
correct master, macOS local suppression must work — today it does not, so while "Mac is
master" the Mac types into *both* machines.

## Journey R — Reverse direction: Mac as master `[~]`

```
Mac is master (Windows is the Active destination)
  ↓
physical input on Mac → forwarded to Windows
  ↓
active CGEventTap callback returns NULL for consumed events → Mac's own apps do NOT see them
```

`[~]` **Implemented** (product-first V1 iteration 2). `platform/src/macos/capture.rs` now
uses an active tap with a raw-FFI trampoline that returns `NULL` to drop a withheld event,
a cross-thread `Arc<AtomicBool>` suppress flag, a `SuppressionGate` port (press/release
symmetry, unit-tested), `TapDisabledBy*` re-arm, and a self-injected-event guard. Fails open
on any callback error. **Not validated on hardware** — the two things needing a real Mac are
whether `NULL` actually drops the event on the running macOS version and whether the
self-inject marker survives `CGEventPost`. See `physical-test-script.md` Round 2.

## Journey 10 — Repeated switching `[~]`

```
A → B → A → B → A → B  (many cycles)
  ↓
no state drift · no stuck keys · no stuck mouse buttons · no duplicate input · no crash
```

`[~]` The single-`Active` invariant is clean in code, the debounce is unit-tested, and the
`drive_two_daemons` example (two real daemons on one host, synthetic input over real
sockets) passes: forwarding follows the active device, a switch halts the previous
direction, and **no duplicate input** — after the iteration-3 self-injection fix, 14 sent =
14 landed exactly, each direction. Still unverified: rapid switching across two *physical*
machines, and whether the connection-ownership concern ("#4") surfaces there. Held-key
release across a switch relies on the `SuppressionGate` press/release symmetry
(unit-tested Windows + macOS).

## Journey 11 — Disconnect / reconnect `[~]`

```
peer disconnects (Wi-Fi off, other daemon quits)
  ↓
local input immediately usable again (suppression released, held keys/buttons released)
  ↓
link state → Reconnecting; discovery keeps re-announcing
  ↓
peer returns → redial → Noise handshake → streaming resumes → switching works again
```

`[~]` Release-on-disconnect, held-input synthesis, discovery-driven redial, and link-state
transitions are implemented and unit-tested. Not validated on two machines. A separate
unresolved concern ("#5", a WebSocket 1006 abnormal close possibly misread by the Flutter
IPC layer as a fatal daemon disconnect) is evidence-blocked pending a real run.

## Journey 12 — UI reflects real daemon state `[~]`

```
daemon truth (devices, which is Active, link state) → IPC state stream → UI
  ↓
UI shows: which device is master ("Using"), Connected / Reconnecting / Disconnected,
paired device list, pairing prompts
```

`[x]` for device list, Active device, pairing, settings — audited live on Windows
(`todos-get-app-working.json` track B/D). `[~]` for the `Reconnecting` / `Disconnected`
link-state transitions driven by a *real* peer drop — only ever seen via mock/unit paths.

## Journey 13 — Restart `[x]`

```
daemon restart
  ↓
ed25519 identity persists (SQLite) · paired devices persist · settings persist
  ↓
discovery re-announces → previously-paired reachable peer is redialed automatically
```

`[x]` SQLite-backed persistence verified across restarts on Windows
(`todos-get-app-working.json` track A, `manual-testing-strategy.md` Tier 0). `DeviceState`
is deliberately not persisted (no stale `Active` on boot).

---

## Priority for the product-first loop

| # | Journey | Status | Priority | Why |
|---|---------|--------|----------|-----|
| — | Clean product-level logging | done | ~~P0~~ | ✅ iteration 1 — `RUST_LOG=flow=debug`, scoped, no dep noise; `[INPUT]/[SWITCH]/[PEER]/[ERROR]` lines |
| R | Mac as master (macOS suppression) | `[~]` | ~~P0~~ | ✅ iteration 2 code-complete + unit-tested; **needs Mac hardware validation** |
| 6–9 | Physical Win↔Mac keyboard/mouse/switch | `[~]` | **P0** | The definition of done; needs the maintainer + two machines (Round 1 + Round 2) |
| 10 | Repeated switching, no drift | `[~]` | P1 | No-dup-input proven on one host (`drive_two_daemons`); physical rapid-switch + "#4" still open |
| 11 | Disconnect / reconnect | `[~]` | P1 | Depends on 6–9; may surface "#5" |
| 5 | Windows ↔ Mac pairing | `[~]` | P1 | One manual step (Pair on the Mac); path already proven Win↔Win |
| 12 | Link-state transitions in UI | `[~]` | P2 | Cosmetic-adjacent; validated once 11 is exercised |

Journeys 1–4, 13 are `[x]` and are regression-checked by `cargo test` + the Flutter e2e
suite, not re-derived here.
