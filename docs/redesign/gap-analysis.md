# Flow 2.0 — Gap analysis (engine ↔ UI ↔ product)

**Status (Track E, E3):** a Status column on every `G-` row records what was fixed (with commit) and what remains and why. Section A and E describe the baseline and are not re-stated.

Track C, milestone C1. Baseline: integration branch before Tracks A/B land (contract 0.1.3, Flutter UI, V1 "baton" model).
Target: `docs/redesign/PLAN.md` (contract 0.2.0) and `docs/adr/0001-controller-controlled-input-model.md`.
This document drives the later tracks; every row names an owner track and a concrete fix.

**Severity**: `S1` blocks the product promise or is a correctness bug; `S2` visibly broken/misleading or required by ADR 0001; `S3` polish / consumer-grade expectations.
**Owner**: `A` engine (control model, settings, dispatcher) · `B` keymaps/remaps · `C` UI · `E` embed/Tauri shell.

## Summary — what matters most

1. The control model itself (roles, active screen, release, layout) has **no representation in the contract or the UI** (G-30…G-38). The V1 UI can only say "Controlling: ‹device›" (`dashboard_section.dart:289-299`) and press "Switch", which the engine implements as flipping a flag after a 400 ms sleep (`service/mod.rs:583-639`).
2. **Six of the nine persisted settings do nothing in the engine** (`share_keyboard`, `share_mouse`, `pointer_sensitivity`, `launch_at_login`, `show_tray_icon`, plus `auto_connect_paired_devices`): they are persisted and echoed, never read (G-10…G-15).
3. The permission flow is fake outside macOS and non-functional on macOS: `request_permission` flips a boolean (`service/mod.rs:1596-1607`); Windows/Linux report `granted: true` unconditionally (`service/mod.rs:1692-1710`). Linux users with no `input` group get silence (G-20…G-23).
4. The shipped UI has a **recording bug**: the shortcut recorder stores `⌘`/`Win` as the key token (`input_section.dart:77`) but the engine's matcher only knows `Meta` (`daemon/src/hotkey/mod.rs:97`) — a ⌘-based shortcut recorded on a Mac never fires (G-16).
5. No diagnostics, no error surfaces for most failures, no accessibility work, one theme choice that skips "System", and the product is called "Cross Device" in 11 UI strings while the binary/repo is "Flow" (G-40…G-60).

---

## A. Engine capabilities vs UI exposure

Commands: `daemon/src/ipc/dispatch.rs:41-105`. Events: `daemon/src/ipc/server.rs` (six `*_changed` events, sent on connect and on `watch` change). Service API: `daemon/src/service/mod.rs`.

| Engine capability | Where | Flutter UI exposure | Verdict |
|---|---|---|---|
| `switch_active_device` | `dispatch.rs:42`, `service/mod.rs:583` | Dashboard "Switch" button (`dashboard_section.dart:419-435`), tray row | OK, but no "switch to *this* computer / return home" (the local device row is also "Switch"-able only when inactive) and no feedback beyond a one-word toast "Switched" (`:430`) |
| `remove_device` | `dispatch.rs:49`, `service/mod.rs:749` | "✕" on Dashboard (`dashboard_section.dart:436-463`) and Devices (`devices_section.dart:112-137`) | Exposed with **no confirmation**, no label/tooltip, 26 px hit target (G-24) |
| `start_pairing` / `cancel_pairing` / `pair_with_candidate` | `dispatch.rs:56-71` | Dashboard "Pair new device", `TrayPairingView`, onboarding `PairStep` | OK; candidate flow has no confirmation code on the initiating side (only the *incoming* dialog shows a fingerprint, `incoming_pairing_request_dialog.dart:51-63`) (G-25) |
| `respond_to_pairing_request` | `dispatch.rs:58` | `IncomingPairingRequestListener` (global) | OK; Material `AlertDialog` look, no timeout countdown although the daemon rejects after 30 s (`service/mod.rs` `PAIRING_DECISION_TIMEOUT`) (G-26) |
| `set_switch_key` | `dispatch.rs:72`, `service/mod.rs:1545` | Recorder + 4 presets (`input_section.dart:61-177`) | Works, but **token mismatch bug** (G-16) and no conflict/validation feedback |
| `update_settings` | `dispatch.rs:79` | General/Devices/Input/Advanced toggles | Round-trips; see settings table below |
| `reset_settings` | `dispatch.rs:86` | "Reset all settings" (`advanced_section.dart:77-86`) | **No confirmation** for a destructive action (G-27) |
| `request_permission` | `dispatch.rs:87`, `service/mod.rs:1596` | Onboarding permission step, Advanced row | Daemon just sets `granted = true` — never opens/asks the OS; Flutter side opens the OS pane via `url_launcher` (`onboarding_flow.dart:108-125`) but the daemon state then lies (G-20) |
| `retry_connection` | `dispatch.rs:91`, `service/mod.rs:1620` | Tray banner "Retry" (`tray_popover.dart:178,185`) | OK; not available in the main window (no link banner there) (G-45) |
| `debug_inject_input` | `dispatch.rs:92` | none (test hook) | Intentional |
| Events: devices/link/pairing/settings/permission/incoming request | `server.rs:130-200` | All consumed (`ipc_daemon_repository.dart`) | OK |
| `Device.last_seen` | `core/src/device/mod.rs:40`; only bumped on switch (`service/mod.rs:604,656,715`) | "last seen 3 min ago" (`devices_section.dart:106`) | **Semantically wrong**: it is "last switched", not "last seen online" (G-17) |
| `hop_note!`/product log + history logger (`storage/history_logger`) | `service/mod.rs:628`, `logging.rs` | none | No way to view/export logs from UI (G-62) |
| `FLOW_DEVICE_NAME`/hostname | `service/mod.rs:1636` | Device name shown, **not editable** | Cannot rename this computer (G-28) |

### Settings: persisted vs honoured by the engine

Searched `daemon/`, `platform/`, `core/` for every `FlowSettings` field outside storage and the settings module itself (`rg` results: only `auto_reconnect` and `debug_logging` have non-storage readers).

| Setting (`core/src/settings/mod.rs:21-29`) | UI control | Honoured by engine? | Evidence | Gap |
|---|---|---|---|---|
| `switch_key` | recorder + presets | **Yes** | `hotkey/runner.rs:57,94,124,182` rebinds live | token bug G-16 |
| `auto_reconnect` | Devices → "Reconnect automatically" | **Yes** | `channel/reconnect.rs:33,67`, `main.rs:593` | none |
| `debug_logging` | Advanced → "Detailed logging" | **Yes** | `logging.rs:188-196` | no log viewer/export |
| `share_keyboard` | Input → "Share keyboard" (`input_section.dart:179-189`) | **No** — no reader anywhere | `rg share_keyboard` → storage + tests only | G-10 (S1: toggle lies) |
| `share_mouse` | Input → "Share mouse" (`:190-200`) | **No** | same | G-10 |
| `pointer_sensitivity` | Input → segmented (`:201-216`) | **No** | same | G-11 |
| `launch_at_login` | General (`general_section.dart:134-144`), default **true** | **No** — nothing registers an autostart entry | no `autostart`/login-item code in repo | G-12 (also: default-on without consent) |
| `show_tray_icon` | General (`:145-157`) | **No** — the tray is created by Flutter regardless of the daemon flag | no reader | G-13 |
| `auto_connect_paired_devices` | Devices → "Connect paired devices" (`devices_section.dart:54-64`) | **No** — `main.rs` discovery loop always calls `dial_if_trusted` | `main.rs` discovery loop | G-14 |
| `appearance` (UI only) | General → Dark/Light (`general_section.dart:162-174`) | n/a (client pref, `ui_providers.dart:14` default **dark**) | no "System" option though the doc comment `ui_providers.dart:11` says system is supported | G-41 |

### UI elements with static or fake data

| Element | Evidence | Gap |
|---|---|---|
| Diagnostics row: "Switch-timing metrics aren't collected yet." | `advanced_section.dart:43-48`; contract says out of scope (`daemon-ipc.md` last section) | G-61: no numbers exist; add `Diagnostics` (contract 0.2.0) |
| Dashboard "Controlling" hero shows `active?.name ?? '—'` and a link label | `dashboard_section.dart:297,320` | Only the *baton* holder; no who-controls-whom, no screen, no latency (G-30) |
| Onboarding "Step N of 4" + 4 hard-coded step labels | `onboarding_flow.dart:25,314` | no role step (G-31) |
| Permission name fallbacks per platform | `platform_chrome.dart:38-40` | OK as fallback; real name comes from daemon |
| Dev harness (`features/harness/dev_harness.dart`) with mock repo | whole file | Dev only; replaced by `?scenario=` mock transport in the new UI |
| "Nearby" candidate caption hard-coded | `pair_step.dart:160` | cosmetic |

---

## B. UX gaps vs a consumer product / Apple HIG

| ID | Area | Gap and evidence | Sev | Owner | Fix | Status |
|---|---|---|---|---|---|---|
| G-40 | Onboarding | 4 steps (Welcome, Permission, Pair, Done) with a technical progress label "Step N of 4" (`onboarding_flow.dart:314`). No role choice; permission is a hard gate that cannot be satisfied in dev (`permission_step.dart:27-33`, `allowSkip` hack). Done step names the key but renders it as plain text (`done_step.dart:50`) | S1 | C | 5 paged steps with progress dots: Welcome → Role → Permissions (OS-specific) → Pair → Done (KeyCaps). Permission step never dead-ends: "Not now" always available with consequences explained | Fixed — Track C `8c68625` |
| G-41 | Appearance | Only Dark/Light; default dark; no System (`general_section.dart:163-166`, `ui_providers.dart:14`) | S2 | C | System / Light / Dark, default System (`prefers-color-scheme`), persisted client-side | Fixed — Track C `8c68625` |
| G-42 | Naming/copy | UI says "Cross Device" (11 occurrences, e.g. `done_step.dart:50`, `tray_popover.dart:262`, `pair_step.dart:75`); vision doc title and Rust crates say Flow; error copy mentions "Flow's background service" (`onboarding_flow.dart:99`, `permission_step.dart:86`) | S2 | C | One name: "Flow". No "daemon/IPC/baton/background service" in user-facing text | Fixed — Track C `8c68625` |
| G-43 | Empty states | Dashboard with only "This device" shows a bare list; Devices section never explains how to add a computer; no first-run "nothing paired" art/CTA | S2 | C | `EmptyState` with illustration + primary "Add a computer" | Fixed — Track C `8c68625` |
| G-44 | Error states | Command errors surface as a raw `e.message` toast (`dashboard_section.dart:431-433`, `devices_section.dart:120-122`); messages are developer strings (`"device not switchable: DeviceId(\"d2\")"`, `core/src/error/mod.rs:15-29`) | S1 | C+A | Stable `code` → friendly copy map in the UI (contract already says message is not matched on); engine keeps `message` as debug detail | Fixed — Track C `8c68625` |
| G-45 | Link banners | Banners exist only in the tray popover (`tray_popover.dart:149-192`); the main window shows a text label ("Reconnecting") with no action (`dashboard_section.dart:234-241,320`). `permissionRequired` banner "Allow" routes nowhere in the main window | S2 | C | Banner component used by main window + tray for all six states per `data-model.md`, with Retry/Cancel/Allow actions wired | Fixed — Track C `8c68625`; "Stop Trying" now `cancel_reconnect` `c66269a` |
| G-46 | Permission guidance per OS | One generic paragraph (`permission_step.dart:66-72`); macOS needs **Accessibility and Input Monitoring**, only one is modelled (`service/mod.rs:1692-1697`); Windows note about elevated apps/UAC missing; Linux has no guidance for `input` group / `/dev/uinput` udev rule / Wayland limits; "Open settings" deep links only mac/Windows (`onboarding_flow.dart:108-125`) | S1 | C+A | Contract: `permission_changed` becomes a list/`kind` of required permissions with per-item `granted`; UI shows per-OS steps with deep links (`x-apple.systempreferences:…Privacy_Accessibility` and `…Privacy_ListenEvent`, `ms-settings:`), Linux copy block with copyable commands | Fixed — `c66269a`: engine reports per-item permissions with structured fix-it steps, deep links, Check Again |
| G-47 | Switch feedback | One-word toast "Switched" (`dashboard_section.dart:430`); no on-screen confirmation on the *controlled* machine, no sound/HUD; user cannot tell where the keyboard went after pressing Scroll Lock | S1 | C+A | macOS-style centered HUD "Now controlling ‹Device›" on every `control_state_changed`, on both machines; honours Reduce Motion | Fixed — HUD Track C `8c68625`; controlled-side `entered` alignment `c66269a` |
| G-48 | Tray | Tray popover is a good base (`tray_popover.dart`) but: shows "Using"/active baton only; `Pair New Device`, `Dashboard`, `Settings` rows; no "Return to this computer", no per-device switch with state glyphs for roles; menu fallback (`tray_menu.dart:100-152`) duplicates logic in Dart | S2 | C+E | Native tray menu (Tauri) + `/tray` popover route; Esc closes, arrow-key navigation; "Return to this computer" always first when remote active | Fixed — `c66269a`: native menu (status, devices, Return, Open, Quit) + popover. Popover/left-click untested on macOS/Windows; Linux is menu-only (appindicator reports no clicks) |
| G-49 | Accessibility | No `Semantics`, tooltips, focus traversal or shortcuts in the Flutter UI (`rg Semantics|Tooltip|FocusTraversal` → none); delete "✕" is a `GestureDetector` over a `Text` (not focusable); custom toggles/segments from `primitives.dart` | S1 | C | Native-semantic HTML (`button`, `role=switch`, `radiogroup`, `dialog`), visible focus ring, full keyboard path, 4.5:1 contrast, `prefers-reduced-motion` | Fixed — Track C `8c68625` |
| G-50 | Keyboard navigation | Recorder is the only keyboard-aware widget (`input_section.dart:115`); no arrow-key sidebar navigation, no ⌘, / Esc handling | S2 | C | Sidebar `role=tablist`-style roving tabindex; Esc closes sheets; ⌘1…⌘6 section shortcuts | Fixed — Track C `8c68625` |
| G-51 | Dark mode / tokens | Palette is custom (`tokens.dart`), "glass" is faked (`glass_surface.dart`), no semantic label/secondaryLabel mapping, not Apple HIG type scale (`FlowType.meta/body/heroTitle` ad hoc sizes 10.5/15/20) | S3 | C | HIG tokens (13 pt body, 22 title, system font stack), vibrancy via `backdrop-filter`, system accent blue + green/orange/red statuses | Fixed — Track C `8c68625` |
| G-52 | Settings structure | 5 flat sections ("Dashboard, General, Devices, Input, Advanced"); "Devices & connection" mixes list + two toggles; no System-Settings-style grouped rows, no descriptions on destructive actions | S3 | C | Sidebar: Home, Devices, Keyboard & Shortcuts, General, Diagnostics; inset-grouped lists | Fixed — Track C `8c68625` |
| G-53 | Destructive confirmations | Remove device and Reset settings act immediately (`dashboard_section.dart:439`, `advanced_section.dart:82`) | S2 | C | `Alert` with destructive button; copy explains consequence ("Flow will stop sharing with ‹Device›; pair again to reconnect") | Fixed — Track C `8c68625` |
| G-54 | Pairing UX | Initiator shows no verification code; candidate list without OS glyph; failed state auto-returns to idle after 1.6 s (contract) so the error is flashed, not readable | S2 | C+A | Sheet with explicit states; code confirmation on both sides; failure stays until dismissed (UI-side hold, engine behaviour unchanged) | Fixed — Track C `8c68625` |
| G-55 | Window chrome | Fake traffic lights/"OS bar" drawn by Flutter for previewing platforms (`window_chrome.dart`, `platform_chrome.dart`) | S3 | C+E | Use real window decorations from Tauri; unified toolbar region via CSS only | Fixed — real window decorations (Tauri) `f0de135` |
| G-56 | Localisation/units | English literals everywhere; relative time helper hand-rolled (`devices_section.dart:150-156`) | S3 | C | Centralised string table + `Intl.RelativeTimeFormat` | Remaining — copy is centralised in `app/ui/src/copy.ts` and relative time uses `Intl`, but there is one language (English); no translation pipeline |
| G-57 | Update/About | No About, version, update check, links to help/privacy | S3 | C+E | About row in General (version from Tauri), "Check for updates" later | Remaining — General → About shows the app name; no update check and no version from Tauri yet |
| G-58 | Rename computer | Device name read-only (G-28) | S3 | A+C | `set_device_name` (deferred; not in 0.2.0) | Remaining — `set_device_name` deliberately deferred (not in contract 0.2) |
| G-59 | Notifications | No system notification when a pairing request arrives while the window is closed | S2 | E | Tauri notification + open window on `incoming_pairing_request_changed` | Fixed with a caveat — `c66269a`: notification while the window is hidden plus a tray item "Review pairing request"; the desktop notification plugin has no click callback, so clicking the notification itself does not open the window |
| G-60 | First-run default | `launch_at_login: true` by default with no consent (`settings/mod.rs:38`) | S2 | A+C | Default false; ask in onboarding "Open Flow when you log in" toggle | Fixed — `c66269a`: default `false`; onboarding Done step offers the toggle |

---

## C. Gaps implied by ADR 0001 and contract 0.2.0

| ID | Gap | Evidence today | Sev | Owner | Fix | Status |
|---|---|---|---|---|---|---|
| G-30 | Active-screen indicator and who-controls-whom | Contract has `Device.state = active` only (`data-model.md`); hero card shows one name (`dashboard_section.dart:289-299`) | S1 | A (event) + C | `ControlState { role, active_screen, controller_device_id }` + `control_state_changed`; Home status card: "You're controlling Office Mac" / "Work PC is controlling this computer" | Fixed — Track A `a1642ea` |
| G-31 | Role choice | No `role` setting (`core/src/settings/mod.rs:21-29`), no UI | S1 | A + C | `role` setting; Onboarding role step; General → Role with explanation and restart-free switching | Fixed — Track A `a1642ea` |
| G-32 | Screen arrangement | No layout model; switching is cyclic only (`service/mod.rs next_switchable_device`) | S1 | A + C | `ScreenLayout`, `set_screen_layout`, `screen_layout_changed`; drag-to-arrange editor with edge snapping on Home | Fixed — Track A `a1642ea` |
| G-33 | Return home / release control | Controlled side cannot ask for control back (this is the headline V1 bug: `hotkey/runner.rs:163` `not_primary`); no UI affordance | S1 | A + C | `release_control` command; "Release control" button on controlled device; "Return home" on controller; `return_home_key` setting + recorder | Fixed — Track A `a1642ea` |
| G-34 | Role-conflict link errors | Two controllers pairing silently misbehave; no error code (`core/src/error/mod.rs` has no variant) | S1 | A + C | `Device.link_error = "role_conflict"`; Devices row shows actionable copy "Both computers are set to share their keyboard. Change one of them to 'Controlled'" with a button that opens General → Role | Fixed — Track A `a1642ea` |
| G-35 | Edge switching settings | None | S2 | A + C | `edge_switching`, `edge_switch_delay_ms` settings; toggle + delay slider in Keyboard & Shortcuts | Fixed — Track A `a1642ea` |
| G-36 | Keymap/remap editing | Mapping tables compiled in (`platform/src/*/translate.rs`, `inject_translate.rs`); `rg keymap|remap` finds no config loading | S2 | B + C | `get_keymap {os}`, `set_remap_rules`, `remap_rules_changed`, `reveal_config_dir`; remap rules editor (from → to, target OS) + "Open keymap folder" | Fixed — Tracks A/B/C; `reveal_config_dir` `c66269a` |
| G-37 | Device metadata | `Device` lacks `role`, `screen`, `last_seen_ms`, `link_error` (`core/src/device/mod.rs:34-41`) | S1 | A | Contract 0.2.0 `Device` additions | Fixed — Track A `a1642ea` |
| G-38 | `switch_to` supersedes `switch_active_device` | alias required (`PLAN.md`) | S2 | A | `switch_to {target: "local"|DeviceId}`; keep alias one release; UI uses only `switch_to` | Fixed — Track A `a1642ea` |
| G-39 | Role-aware wording and UI | "Primary/Secondary", "Using" (`tray_popover.dart:303`) | S2 | C | Controller/Controlled expressed as "Shares keyboard & mouse" / "Controlled by another computer" | Fixed — Track C `8c68625` |

---

## D. Engine-side defects and missing capabilities found while reading

| ID | Gap | Evidence | Sev | Owner | Fix | Status |
|---|---|---|---|---|---|---|
| G-10 | `share_keyboard`/`share_mouse` ignored | no reader (see settings table) | S1 | A | Router drops keyboard/mouse events when off; test per toggle | Fixed — Track A `a1642ea` |
| G-11 | `pointer_sensitivity` ignored | same | S2 | A | Scale relative deltas on the controller side before forwarding | Fixed — Track A `a1642ea` |
| G-12 | `launch_at_login` ignored | none | S2 | E | Tauri autostart plugin driven by the setting (macOS login item, Windows registry, Linux `.desktop`) | Fixed — `c66269a` via tauri-plugin-autostart (verified on Linux: `~/.config/autostart/Flow.desktop`; macOS/Windows unverified) |
| G-13 | `show_tray_icon` ignored | none | S2 | E | Tauri tray `set_visible`; keep window reachable if hidden | Fixed — `c66269a` (`TrayIcon::set_visible`; with the icon off, closing the window quits). Not visually verified on any OS |
| G-14 | `auto_connect_paired_devices` ignored | discovery loop dials unconditionally | S2 | A | Gate `dial_if_trusted` on the setting | Fixed — `c66269a`: `dial_if_trusted` gated; off means Flow only accepts connections |
| G-15 | Unused setting round-trips masquerade as features | all of the above | S1 | A+C | Until honoured, UI hides the control (new UI does not render `share_*`/sensitivity until Track A lands them — see UI note below) | Partly — the engine honours `share_*`/sensitivity (Track A); the UI still does not render those controls |
| G-16 | Shortcut recorder token mismatch | `input_section.dart:77` emits `⌘`/`Win`; `hotkey/mod.rs:97` only parses `Meta` | S1 | C | New recorder emits canonical tokens (`Ctrl Alt Shift Meta`, key names from `core/src/protocol/key_names.rs`); render glyphs only in KeyCap | Fixed — Track C `8c68625` |
| G-17 | `last_seen` is "last switched" | `service/mod.rs:604,656,715` only | S2 | A | Update on every heartbeat/connect/disconnect → `last_seen_ms` | Fixed — `c66269a`: `last_seen` updates on heartbeat (throttled 5 s), not on switch |
| G-18 | Link state is global, not per device | `DaemonLinkState` single value | S2 | A | Per-device `state` + `link_error`; global state only for engine health | Remaining — the global link state is still one value; per-device `state` and `link_error` exist and drive per-device UI |
| G-19 | Switch has a fixed 400 ms sleep | `service/mod.rs:34 SWITCH_DEBOUNCE`, `:600` | S2 | A | Remove; switching must be immediate and measured (feeds `Diagnostics.switch_latency_ms_*`) | Fixed — Track A `a1642ea` |
| G-20 | `request_permission` is fake | `service/mod.rs:1596-1607` | S1 | A+E | Engine probes the real permission (`AXIsProcessTrusted`, IOHID listen access, uinput/evdev open) and re-polls; command opens the right OS pane | Fixed — `c66269a`: real probes and prompts (macOS `AXIsProcessTrustedWithOptions`, `IOHIDRequestAccess`; Linux evdev/uinput). macOS code compiled/tested only on Linux CI — unverified on a Mac |
| G-21 | Windows/Linux permission "granted" hard-coded | `service/mod.rs:1692-1710` | S1 | A | Linux: check `/dev/uinput` and `/dev/input/event*` readability → report real missing state with remediation key | Fixed — `c66269a` (Linux real; Windows reports nothing to grant) |
| G-22 | Single permission modelled | `PermissionStatus {name, granted}` | S2 | A | List of `{kind, name, granted, help_url?}`; macOS: accessibility + input_monitoring | Fixed — `c66269a`: `PermissionStatus.items` |
| G-23 | Wayland not detected | no mention in platform/linux | S2 | A | Report `permission_required` with Wayland-specific reason when capture is impossible | Remaining — Wayland is mentioned in the guidance but not detected |
| G-24 | Remove device is irreversible and unlabeled | see section A | S2 | C | Confirmation alert (G-53) | Fixed — Track C `8c68625` |
| G-25 | No verification code on initiator side | `pairing.dart` PairingSession has no fingerprint | S2 | A | `PairingSession.code` once `requesting` | Fixed — `c66269a`: `PairingSession.code`, same value as the other side's fingerprint |
| G-26 | No countdown for incoming request | 30 s timeout hidden | S3 | C | Show "Expires in 24 s" progress | Fixed — Track C `8c68625` |
| G-27 | Reset has no confirmation | `advanced_section.dart:82` | S2 | C | Alert (G-53) | Fixed — Track C `8c68625` |
| G-28 | No rename of this computer | n/a | S3 | A | later | Remaining — deferred with G-58 |
| G-61 | No diagnostics | `Diagnostics` absent (only text logs) | S2 | A | `Diagnostics { switch_latency_ms_p50/p95, events_forwarded, events_dropped, uptime_s }` + `diagnostics_changed` + `export_diagnostics` | Fixed — Track A `a1642ea` |
| G-62 | No log viewing/export | `logging.rs` only writes | S3 | A+E | `export_diagnostics` saves a zip/text bundle via native save dialog | Fixed — `c66269a`: `export_diagnostics` writes a JSON bundle (log tail included) under the data dir and the UI shows the path; no native save dialog |

### UI note on unhonoured settings

Until Track A makes them real, the new UI **renders only settings the engine honours or that the contract 0.2.0 adds**: switch key, return-home key, edge switching, role, auto-reconnect, auto-connect paired devices, launch at login, show in menu bar, debug logging. `share_keyboard`, `share_mouse`, `pointer_sensitivity` are modelled in the TypeScript types and mock but not rendered (G-10/11/15). The mock honours them so tests exist the day the engine does.

---

## E. Parity checklist the new UI must meet (inherits V1 behaviours worth keeping)

| V1 behaviour | Keep? | New location |
|---|---|---|
| Tray popover with status dot + banner + device switch | Keep, redesign | `/tray` route + native tray menu |
| Pairing from tray and onboarding | Keep | Pairing `Sheet` reachable from Devices, Home empty state, onboarding |
| Incoming pairing prompt on any screen | Keep | Global `Sheet` |
| Presets for switch key | Keep | Recorder + preset pop-up |
| Dev harness | Replace | `?scenario=` mock transport |
| `FLOW_ENV` permission skip | Replace | "Not now" in onboarding, banner thereafter |

## F. Severity-ranked work list

| Order | IDs | Track |
|---|---|---|
| 1 | G-30, G-31, G-33, G-34, G-37, G-38, G-19 | A (contract 0.2.0 + router) |
| 2 | G-10, G-14, G-20, G-21, G-22, G-16 | A (honour settings, real permissions), C (recorder) |
| 3 | G-32, G-35, G-47, G-44, G-46 | A + C (layout, HUD, copy, permission guidance) |
| 4 | G-36 | B + C (keymaps) |
| 5 | G-12, G-13, G-48, G-59 | E (autostart, tray, notifications) |
| 6 | G-40…G-60 remainder | C (this track delivers: onboarding, design system, a11y, empty/error states, banners) |
