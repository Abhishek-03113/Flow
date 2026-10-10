# Flow 2.0 — User Journeys

Derived from [`docs/product/vision.md`](../product/vision.md), the
[controller/controlled model (ADR 0001)](../adr/0001-controller-controlled-input-model.md)
and the [redesign plan](../redesign/PLAN.md). Each journey lists what verifies it today.

> **In one sentence:** the keyboard and mouse of one computer (the controller) drive one or
> more other computers (controlled), the switch hotkey, a screen edge or the UI moves between
> screens, and the controller's own keyboard can always bring control back.

## Status legend

- `[x]` verified automatically: engine logic through the real engine on virtual operating
  systems ([`harness.md`](harness.md) scenarios, every OS pair), plus unit and integration tests.
- `[~]` verified automatically above the OS, **not yet on physical machines**: the platform
  adapters (Windows hooks, macOS event tap, `SendInput`/`CGEventPost`, evdev/uinput) are the
  remaining risk. The gate is [`physical-test-script.md`](physical-test-script.md).
- `[ ]` broken or missing.

Every `[~]` below shares that one reason. Everything above the adapters is the same code in
the harness and on hardware.

---

## 1 — Start Flow `[x]`

```
start the app (engine embedded) or the headless flow-daemon
  ↓
SQLite state: settings (incl. role), devices, layout, identity, history
  ↓
discovery + peer listener; the controller captures, a controlled device does not
```

Verified: `daemon/tests/two_engines.rs`, the harness (every world starts real engines),
Tauri embedding (track E). Startup misconfiguration exits with a one-line reason.

## 2 — Choose a role `[x]`

```
onboarding: "Share this computer's keyboard & mouse" (controller)
            "Control this computer from another one" (controlled)
  ↓
role persisted; changeable in Settings (links renegotiate)
```

Two controllers or two controlled devices refuse to link with `role_conflict`, shown on the
device. Harness: `role-conflict-controllers`, `role-conflict-controlled`. UI: onboarding and
settings Vitest/Playwright suites.

## 3 — Discover and pair `[x]` (Windows ↔ Mac `[~]`)

```
UDP announce → pairing candidate → Pair → Noise handshake → consent on the other side
  ↓
both persist the peer; Hello exchanges role, OS and screen; link live
```

Every harness world pairs its devices through real discovery, pairing and consent over
loopback. Physical: Round 0.

## 4 — Control the other computer `[~]`

```
controller presses the switch hotkey (default Scroll Lock; F13 on a Mac keyboard)
  ↓
suppress locally → Enter → keys, pointer, buttons, scroll forwarded to that screen only
  ↓
injected as the target OS's own key codes; remap rules for that OS applied
```

Harness: `basic-switch`, `no-local-leak-while-remote`, `remap-per-target-os`,
`sharing-settings`, `pointer-sensitivity`, `hotkey-chord-never-reaches-an-os`. Physical:
Round 1 steps 1–4.

## 5 — Regain control `[~]`

```
the same hotkey on the controller's own keyboard → Leave → un-suppress → home
(or the return-home key, or "Release control" on the controlled machine)
```

The bug that motivated ADR 0001: fixed by construction (only the controller switches, and its
capture sees its keyboard while hidden). Harness: `basic-switch`, `return-home-key`,
`release-request`. Physical: Round 1 steps 5 and 11.

## 6 — A controlled machine stays its own `[~]`

```
the controlled machine's own keyboard/trackpad → its own apps only, never forwarded
```

Harness: `controlled-input-never-forwarded`. Physical: Round 1 step 6.

## 7 — Edge switching and arrangement `[~]`

```
pointer through an edge that touches a neighbour → that screen (and back), entry point mapped
```

Default arrangement puts controlled screens to the right; the user can rearrange. Harness:
`edge-switching-layout` (three screens), `custom-layout`, `edge-switching-off`. Physical:
Round 1 step 7.

## 8 — Several computers `[x]`

```
one controller, N controlled: hotkey cycles Local → A → B → Local; switch_to jumps anywhere
```

Harness: `three-device-cycle`, `edge-switching-layout` (OS triples).

## 9 — Repeated switching, held input `[~]`

```
many switches with typing in between: no drift, no stuck keys or buttons, no duplicates
```

Up follows down for keys and buttons; keys held at a switch are released on the side that
saw the press. Harness: `rapid-switching` (200 toggles), `held-key-across-switch`,
`held-button-across-switch`, `hotkey-autorepeat-debounce`. Physical: Round 1 steps 8–10.

## 10 — Disconnect and reconnect `[~]`

```
link lost while remote → controller returns home and un-suppresses at once
controlled side releases held input on disconnect or heartbeat silence
peer returns → redial → switching works again
```

Harness: `disconnect-controlled-while-remote`, `disconnect-controller-while-remote`;
heartbeat silence: `daemon/tests/link_roles.rs`. Physical: Round 1 steps 12–13.

## 11 — The UI shows the truth `[x]`

```
engine state (devices, roles, link errors, active screen, control state) → event stream → UI
```

UI suites against the mock and WebSocket transports; the UI against live harness daemons is
track G (it uses `flow-harness up`, see [`harness.md`](harness.md#up-mode-for-ui-e2e)).

## 12 — Restart `[x]`

Identity, pairings, role, layout and settings persist; previously paired peers are redialed.
Harness: the `reconnect` step restarts an engine on its old data.

## 13 — The controller is a Mac `[~]`

The macOS event tap must drop the Mac's own events while remote and must not re-capture what
Flow injects. Harness covers every Mac-controller pair above the adapter; the tap itself is
Physical Round 2.
