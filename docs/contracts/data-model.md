# Data Model

Contract version: **0.1.0** (see `CHANGELOG.md`). Every entity below has a Dart type (in `flutter/lib/domain/`) and the JSON shape it would take on the wire once the Control Link medium exists. Field names are `snake_case` on the wire and `camelCase` in Dart, converted 1:1.

## `Device`

A computer the daemon knows about — paired or in the process of pairing.

```dart
class Device {
  final String id;
  final String name;
  final HostOs os;
  final DeviceState state;
  final DateTime lastSeen;
}

enum HostOs { macos, windows, linux }
```

```json
{
  "id": "d2",
  "name": "Work Laptop",
  "os": "windows",
  "state": "inactive",
  "last_seen": "2026-08-25T06:58:00Z"
}
```

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

```dart
enum DaemonLinkState { connected, connecting, reconnecting, disconnected, error, permissionRequired }
```

| Variant | Meaning | UI treatment |
|---|---|---|
| `connected` | Daemon reachable, active device receiving input normally. | Green dot, no banner. |
| `connecting` | Initial connection to the daemon still being established. | Amber dot (pulsing), no banner. |
| `reconnecting` | Was connected, lost the link, retrying automatically. | Amber dot (pulsing), banner with **Cancel**. |
| `disconnected` | Daemon unreachable and not retrying (or the active device dropped and nothing has taken over). | Gray dot, banner with **Retry**. |
| `error` | Daemon reachable but input sharing is failing for a reason other than permission (e.g. injection failing). | Red dot, banner with **Retry**. |
| `permissionRequired` | The daemon needs an OS input-capture permission (macOS Accessibility, Windows input access, Linux input device access) before it can do anything. | Red dot, banner with **Allow**, routes to the permission step. |

## Pairing

```dart
enum PairingStage { idle, searching, found, requesting, paired, failed }

class PairingCandidate {
  final String id;
  final String name;
  final HostOs os;
}

class PairingSession {
  final PairingStage stage;
  final List<PairingCandidate> candidates; // populated once stage >= found
  final String? targetName;                // set once stage >= requesting
  final String? error;                     // set only when stage == failed
}
```

```json
{
  "stage": "requesting",
  "candidates": [{ "id": "cand-1", "name": "Office Mac Mini", "os": "macos" }],
  "target_name": "Office Mac Mini",
  "error": null
}
```

State machine and transitions are in `daemon-ipc.md`.

## `SwitchKeyBinding`

The shortcut that switches the active device (`docs/product/vision.md` §12).

```dart
class SwitchKeyBinding {
  final String label;        // human-readable, e.g. "Scroll Lock", "Ctrl + Shift + Space"
  final List<String> keys;   // ordered key tokens, e.g. ["Ctrl", "Shift", "Space"]
}
```

```json
{ "label": "Ctrl + Shift + Space", "keys": ["Ctrl", "Shift", "Space"] }
```

`keys` tokens are platform-neutral strings (`"Ctrl"`, `"Alt"`, `"Shift"`, `"Meta"`, `"ScrollLock"`, `"Pause"`, `"F13"`, single characters, …). Rendering a platform-correct glyph (e.g. `⌘` on macOS for `"Meta"`) is a UI concern, not part of the contract. The four built-in presets are `Scroll Lock`, `Pause`, `F13`, `Ctrl + Shift + Space`; a custom binding is recorded live and carries whatever tokens were pressed.

## `FlowSettings`

```dart
enum PointerSensitivity { low, normal, high }

class FlowSettings {
  final bool launchAtLogin;
  final bool showTrayIcon;
  final bool autoReconnect;
  final bool autoConnectPairedDevices;
  final bool shareKeyboard;
  final bool shareMouse;
  final bool debugLogging;
  final PointerSensitivity pointerSensitivity;
  final SwitchKeyBinding switchKey;
}
```

```json
{
  "launch_at_login": true,
  "show_tray_icon": true,
  "auto_reconnect": true,
  "auto_connect_paired_devices": true,
  "share_keyboard": true,
  "share_mouse": true,
  "debug_logging": false,
  "pointer_sensitivity": "normal",
  "switch_key": { "label": "Scroll Lock", "keys": ["ScrollLock"] }
}
```

Every field is independently updatable — see the `update_settings` command in `daemon-ipc.md`, which takes a partial patch rather than the whole object.

## `PermissionStatus`

Surfaced during onboarding and in the Advanced settings section.

```dart
class PermissionStatus {
  final String name;    // platform-specific, e.g. "Accessibility access"
  final bool granted;
}
```

```json
{ "name": "Accessibility access", "granted": false }
```

`name` is daemon-supplied (it already knows the platform it's running on) rather than derived client-side, so the UI never hardcodes per-OS permission copy.
