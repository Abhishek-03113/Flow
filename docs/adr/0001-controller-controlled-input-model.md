# ADR 0001 — Controller / Controlled input model (replaces the symmetric "ownership baton")

- Status: Accepted
- Date: 2026-10-10
- Reference: [deskflow](https://github.com/deskflow/deskflow) server/client model (`Server`, `ClientProxy`, `enter`/`leave`/`keyDown`/`keyUp`/`mouseMove`/`mouseRelativeMove` protocol)

## Context

Flow V1 models control as a symmetric *baton*: both daemons run input capture, and
`ChannelMessage::OwnershipChanged` moves "Primary" from one side to the other. Whichever
side is Primary captures *its own* physical input and forwards it; the Secondary injects.

That model is the root cause of the bugs reported from real Windows → macOS use:

| Symptom | Cause in the baton model |
|---|---|
| Windows (keyboard+mouse attached) switches to Mac, then **cannot regain control** | After the handoff the *Mac* is Primary. Windows is Secondary and `spawn_pipeline_switch_filter` explicitly ignores the switch key on a Secondary (`switch_ignored reason=not_primary`). The only device that can switch back is the one with no keyboard. |
| After regaining control, **Mac input flows back to Windows** | The baton moved to the Mac, so the Mac's own trackpad/keyboard became the captured-and-forwarded source. Ownership of *the machine with the peripherals* was never the concept being modelled. |
| Windows controlling Mac, **some keystrokes/mouse moves leak into Windows** | Two independent capture instances (hotkey runner + pipeline) with different hook order; suppression toggled asynchronously after the role flip; key-up events of keys pressed before a switch delivered to the wrong side; no "inject only while entered" guard on the receiver. |

## Decision

Adopt deskflow's asymmetric model.

### Roles (static per device, not a baton)

- **Controller** (deskflow *server*) — the machine whose physical keyboard and mouse are shared.
  It is the only side that ever captures input for forwarding and the only side that can switch.
- **Controlled** (deskflow *client*) — a machine driven by a controller. It **never** captures for
  forwarding. Its own local keyboard/trackpad keep working locally and are never sent anywhere.

`role` is a persisted setting (`controller` | `controlled`), chosen in onboarding
("Share this computer's keyboard & mouse" / "Control this computer from another one"), changeable in Settings.
A controller may drive N controlled devices (screens). A controlled device accepts exactly one controller at a time.
A link between two controllers or two controlled devices is refused with reason `role_conflict` (surfaced in UI).

### Active screen lives only on the controller

```
ActiveScreen = Local | Remote(DeviceId)
```

The controller owns this state machine; the controlled side only learns about it via `Enter`/`Leave`.

| Controller state | Local OS sees physical input? | Forwarded? |
|---|---|---|
| `Local` | yes (suppression off) | no |
| `Remote(d)` | **no** (suppression on) | yes, to `d` only |

### Switching

All switching is decided on the controller:
1. **Switch hotkey** (default Scroll Lock) — cycles `Local → d1 → d2 → … → Local`. Detected on the
   controller's single capture stream, which sees physical input even while it is suppressed, so
   *returning home always works*.
2. **Return-home hotkey** (optional, configurable) — jumps straight to `Local`.
3. **Screen edges** — with a screen arrangement (deskflow layout: each screen has neighbours left/right/top/bottom),
   the controller tracks a virtual cursor on the remote screen (controlled side reports its screen size in `Hello`);
   crossing an edge with a neighbour switches. Edge switching can be disabled.
4. **UI / control API** — `switch_to { device_id | "local" }` on the controller. On a controlled device the UI
   offers "Release control" which sends `ReleaseRequest` to the controller (controller decides; it returns to `Local`).

### Wire protocol v2 (inside the existing Noise channel)

| Message | Direction | Meaning |
|---|---|---|
| `Hello { protocol_version, role, os, screen: { width, height }, device_name }` | both, first frame | capability + role exchange; role conflict → `Goodbye{reason}` |
| `Enter { seq, x, y, held_modifiers }` | ctl → ctd | controlled side becomes the input target; warp cursor to (x,y) |
| `Leave { seq }` | ctl → ctd | stop being the target; controlled side **must release every held key/button** |
| `Input { seq, event }` | ctl → ctd | only valid between `Enter` and `Leave`; dropped otherwise (defence in depth) |
| `Arrangement { controller, controlled }` | ctl → ctd | where the two screens sit (rects in the controller's layout); sent on link-up and on layout change (`protocol_version = 3`) |
| `ScreenInfo { width, height }` | ctd → ctl | resolution change |
| `ReleaseRequest` | ctd → ctl | user asked to get their machine back |
| `Heartbeat` / `Goodbye { reason }` | both | liveness / orderly close |

`seq` is a per-connection monotonically increasing counter (replay guard kept).

### Leak-proofing invariants (must be enforced and tested)

1. **Single capture instance per process.** One `InputRouter` owns capture, hotkey matching, suppression and
   routing. The standalone hotkey runner and per-pipeline capture are removed.
2. **Suppress before forward.** On `Local → Remote(d)`: enable suppression synchronously (inside the capture
   thread's decision path, via an atomic read in the hook/tap callback), *then* send `Enter`, *then* forward.
   On `Remote(d) → Local`: send `Leave`, then disable suppression.
3. **Up follows down.** Every KeyUp / ButtonUp is routed to the same destination its KeyDown / ButtonDown went to,
   regardless of the active screen at release time. Keys physically held at switch time are released on the side
   they were pressed on (synthesised release), never delivered as orphan ups.
4. **Hotkey events never reach any OS.** The full chord (down and up) is consumed.
5. **Controlled side injects only while entered**, and releases all on `Leave`, on disconnect, and on heartbeat timeout.
6. **Disconnect always returns home.** Link loss while `Remote(d)` → controller goes `Local` and un-suppresses.
7. **Platform taps self-heal.** macOS: re-enable the event tap on `kCGEventTapDisabledByTimeout/UserInput`.
   Windows: ignore `LLKHF_INJECTED`/`LLMHF_INJECTED` events and the cursor-recenter `SetCursorPos` echo.

### What is removed

`OwnershipChanged`, `OwnershipHandle` generations, `resolve_ownership`, Primary/Secondary `InputRole`
in the pipeline, `spawn_pipeline_switch_filter`, and the standalone `hotkey::runner` capture.

## Consequences

- Fixes "can't regain control" by construction: the controller's keyboard always reaches the controller's router.
- Fixes "Mac input flows back": controlled devices never capture.
- Multi-device (3+) becomes natural (`Remote(d)` over a set of links), as does deskflow-style screen arrangement.
- Breaking change to the daemon↔daemon wire protocol (`protocol_version = 2` then 3); V1 peers are refused with a clear message.
- The UI needs a role choice, a screen-arrangement editor and an active-screen indicator (see `docs/redesign/PLAN.md`).
