# Data Model

Contract version: **0.2.1** (see `CHANGELOG.md`). Every entity below is a JSON shape on the wire, `snake_case`; the TypeScript UI mirrors them 1:1 as `camelCase` types in [`app/ui/src/client/types.ts`](../../app/ui/src/client/types.ts) (the single key mapper is `mapper.ts`) and the engine as serde types in `core/src`.

## `Device`

A computer the daemon knows about — paired or in the process of pairing.

```json
{
  "id": "d2",
  "name": "Work Laptop",
  "os": "windows",
  "state": "inactive",
  "last_seen": "2026-08-25T06:58:00Z",
  "last_seen_ms": 1787641080000,
  "role": "controlled",
  "screen": { "width": 1920, "height": 1080 },
  "link_error": null,
  "is_local": false
}
```

Since 0.2.0: `role` and `screen` describe the peer and are `null` until it said hello on a live link; `link_error` is why the last link attempt failed and `null` otherwise — one of `role_conflict` (same role, or a controlled computer that already has a controller), `protocol_mismatch` (other protocol version), `protocol_error` (unexpected message, or a refusal reason this version doesn't know), `hello_timeout`, `connection_lost` (closed before the link came up), `injector_unavailable` (the controlled computer could not start injecting input); `flow_core::link::link_error` is the list and clients treat an unknown code as `protocol_error`; `last_seen_ms` is `last_seen` in epoch milliseconds. Exactly one device is `active`: the one whose screen the controller's input currently drives. `is_local` is true for this computer (always `d1`). Since 0.2.1 `last_seen` means *last heard from* (heartbeat), not last switched to.

### `DeviceState`

Deliberately identical to `flow_core::device::DeviceState` (`core/src/device/mod.rs`) — same six variants, same meaning, `snake_case` on the wire. The daemon serializes its existing state enum straight into this field; no translation layer.

| Variant | Meaning |
|---|---|
| `pairing` | Pairing handshake in progress with this device. |
| `connected` | Paired and reachable, but not the current input destination. |
| `active` | Currently receiving keyboard/mouse input — at most one device is `active` at a time (this machine, "This device", is `active` by default when nothing else is). |
| `inactive` | Paired, reachable, not active — the state most devices sit in day to day. Exactly what the UI shows as "Connected" with a switch action available. |
| `disconnected` | Paired but not currently reachable (offline, out of network range). |
| `error` | Reachable but the link is unhealthy (see `DaemonLinkState` for *why*, e.g. a permission problem on that device). |

`connected` vs `inactive` is a Rust-side nuance worth flagging: `connected` is the transient "just paired/reconnected, not yet confirmed steady" state; once the daemon considers the link steady it moves the device to `inactive` (or `active`, if it's the one receiving input). The UI treats both as "reachable, switchable" — see `daemon-ipc.md`'s device list rendering rule.

## `DaemonLinkState`

Health of *this machine's* connection to the daemon and, by extension, the active input link. This is a single top-level value, not per-device — it's what drives the tray popover's status dot and banner.

| Variant | Meaning | UI treatment |
|---|---|---|
| `connected` | Daemon reachable, active device receiving input normally. | Green dot, no banner. |
| `connecting` | Initial connection to the daemon still being established. | Amber dot (pulsing), no banner. |
| `reconnecting` | Was connected, lost the link, retrying automatically. | Amber dot (pulsing), banner with **Cancel**. |
| `disconnected` | Daemon unreachable and not retrying (or the active device dropped and nothing has taken over). | Gray dot, banner with **Retry**. |
| `error` | Daemon reachable but input sharing is failing for a reason other than permission (e.g. injection failing). | Red dot, banner with **Retry**. |
| `permissionRequired` | The daemon needs an OS input-capture permission (macOS Accessibility, Windows input access, Linux input device access) before it can do anything. | Red dot, banner with **Allow**, routes to the permission step. |

## Pairing

```json
{
  "stage": "requesting",
  "candidates": [{ "id": "cand-1", "name": "Office Mac Mini", "os": "macos" }],
  "target_name": "Office Mac Mini",
  "error": null,
  "code": "3f2a 91c4 8d10 6b57"
}
```

`code` (0.2.1) is the verification code shown on both computers while `requesting`; the other side's `IncomingPairingRequest.fingerprint` is the same string. State machine and transitions are in `daemon-ipc.md`.

## `IncomingPairingRequest`

Another device that has connected to this daemon's peer listener and is
waiting for the local user to accept or reject it. Independent of
`PairingSession` (which models only the *initiating* side); surfaced to
the UI one at a time as `incoming_pairing_request_changed` — see
`daemon-ipc.md`.

```json
{
  "request_id": "ipr-9f2c1a...",
  "device_name": "Abhishek's Windows",
  "device_os": "windows",
  "fingerprint": "3f2a 91c4 8d10 6b57",
  "address": "192.168.0.103"
}
```

## `SwitchKeyBinding`

The shortcut that switches the active device (`docs/product/vision.md` §12).

```json
{ "label": "Ctrl + Shift + Space", "keys": ["Ctrl", "Shift", "Space"] }
```

`keys` tokens are platform-neutral strings (`"Ctrl"`, `"Alt"`, `"Shift"`, `"Meta"`, `"ScrollLock"`, `"Pause"`, `"F13"`, single characters, …). Rendering a platform-correct glyph (e.g. `⌘` on macOS for `"Meta"`) is a UI concern, not part of the contract. The four built-in presets are `Scroll Lock`, `Pause`, `F13`, `Ctrl + Shift + Space`; a custom binding is recorded live and carries whatever tokens were pressed.

A binding (switch or return-home) must have at least one key (`invalid_switch_key`). If any of its keys is used for typing (a letter, digit, `Space`, punctuation, ...), it must also hold `Ctrl`, `Alt` or `Meta`; `Shift` alone doesn't count (`switch_key_needs_modifier`). `ScrollLock`, `Pause` and `F1`–`F24` work on their own. `return_home_key` must not press the same keys as `switch_key`, in any order or spelling (`switch_key_conflict`). Only the bindings a command sets are checked, so a binding stored by an older version keeps working.

## `FlowSettings`

```json
{
  "launch_at_login": false,
  "show_tray_icon": true,
  "auto_reconnect": true,
  "auto_connect_paired_devices": true,
  "share_keyboard": true,
  "share_mouse": true,
  "debug_logging": false,
  "pointer_sensitivity": "normal",
  "switch_key": { "label": "Scroll Lock", "keys": ["ScrollLock"] },
  "role": "controller",
  "return_home_key": null,
  "edge_switching": true,
  "edge_switch_delay_ms": 0,
  "keymap_overrides_path": "/home/me/.config/flow-daemon/keymaps"
}
```

0.2.0 fields: `role` (`controller` shares its keyboard and mouse, `controlled` is driven by a controller), `return_home_key` (a `SwitchKeyBinding` that jumps straight back to this computer, or `null`), `edge_switching` / `edge_switch_delay_ms` (switch by pushing the pointer through a screen edge), `keymap_overrides_path` (read-only: where user keymap and remap files live). `SettingsPatch` may set all of them except `keymap_overrides_path`; `"return_home_key": null` clears the binding. `share_keyboard`, `share_mouse` and `pointer_sensitivity` are applied by the controller when forwarding (low 0.6x, normal 1x, high 1.6x).

Every field is independently updatable — see the `update_settings` command in `daemon-ipc.md`, which takes a partial patch rather than the whole object.

## `PermissionStatus`

Surfaced during onboarding, in the permission sheet and in Diagnostics.

```json
{
  "name": "Accessibility and Input Monitoring",
  "granted": false,
  "items": [
    {
      "name": "Accessibility",
      "granted": false,
      "how_to_fix": {
        "summary": "Flow needs Accessibility access to type and click for you.",
        "steps": [
          { "text": "Open System Settings › Privacy & Security › Accessibility.", "command": null },
          { "text": "Turn on Flow.", "command": null }
        ]
      }
    },
    { "name": "Input Monitoring", "granted": true, "how_to_fix": null }
  ],
  "one_click_setup": false,
  "restart_required": false
}
```

`items` is what the OS requires, probed for real every ~2 s: macOS Accessibility and Input Monitoring (separately), Linux reading `/dev/input/event*` and creating the virtual device (`/dev/uinput`), Windows nothing (`items: []`, `granted: true`). On Linux, while ufw or firewalld is active and Flow's setup has not opened its ports, a third item `Incoming connections` is listed. `one_click_setup` is true when `request_permission` can grant every missing item by itself behind one administrator prompt (Linux with polkit). `restart_required` is true when a permission was granted while Flow ran and the OS applies it only to a fresh process (macOS Input Monitoring); the desktop app answers the shell-only command `restart_app` by relaunching. `granted` is true when every item is. `how_to_fix` is `null` while granted; `command` is a shell command to copy. Names and text are engine-supplied, so the UI hardcodes no per-OS copy. `open_permission_settings {name}` answers `{ "url": … }` for the item's OS settings page (`null` where there is none).

## `ControlState` (0.2.0)

```json
{ "role": "controller", "active_screen": "local", "controller_device_id": null, "entered": false }
```

`active_screen` is `"local"` or the id of the device the controller's input drives; **always `"local"` on a controlled device**, where `entered` says whether the controller is driving it. `controller_device_id` is the linked controller, on a controlled device. `entered` is true on a controlled device while the controller is driving it.

## `ScreenLayout` (0.2.0)

```json
{ "screens": [
  { "device_id": "d1", "x": 0, "y": 0, "width": 1920, "height": 1080 },
  { "device_id": "pk:ab12…", "x": 1920, "y": 0, "width": 1440, "height": 900 }
] }
```

Screens are rectangles in one shared virtual space (`d1` is this computer); two screens are neighbours where their edges touch. Persisted on the controller. On a controlled device it is instead the controller's arrangement (read-only, not persisted): `d1` and the controller, as the controller placed them. Default: each controlled screen is placed to the right of everything already placed, in connection order.

## `Diagnostics` (0.2.0)

```json
{ "switch_latency_ms_p50": 0.04, "switch_latency_ms_p95": 0.09, "events_forwarded": 1203, "events_dropped": 0, "uptime_s": 3600 }
```

Latencies are `null` until a switch happened. `events_dropped` includes input a controlled device refused (outside `Enter`..`Leave`, replays).

## `RemapRule` (0.2.0)

```json
{ "from": "Ctrl", "to": "Meta", "when_target_os": "macos" }
```

`from`/`to` are two key names or two modifier tokens; `when_target_os` is `"macos" | "windows" | "linux" | null` (all).
