# Feature sprint

The open features and improvements after the Flow 2.0 redesign. They come from the review
(`docs/redesign/review.md`, ids `H-…`) and the UI/engine gap analysis
(`docs/redesign/gap-analysis.md`, ids `G-…`). Bugs are being fixed separately and are not listed here.

## Features

### 1. Detect Wayland (G-23)
On a Wayland session, capturing input can silently fail. The engine should detect Wayland
(`XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`) and report it as a permission item, so the UI can show a
clear reason and what to do (for example, use an X11 session). Today Wayland is only mentioned in the
guidance text.
- Area: `platform/src/permissions.rs`, `PermissionStatus.items`, onboarding permission step.

### 2. Version display and update check (G-57)
General → About shows only the app name.
- Show the app version from Tauri (`app.package_info().version`).
- Add "Check for updates" using `tauri-plugin-updater`. This depends on signed release artifacts
  (see 5).
- Area: `app/src-tauri`, `app/ui/src/features/general`.

### 3. Rename a computer (G-58, G-28)
Device names are read-only.
- Add a `set_device_name {name}` command that persists the name and sends it to paired peers in the
  link handshake.
- Add a rename field for "This computer" on the Computers page.
- Contract change: bump to 0.3.0.
- Area: `daemon/src/service`, protocol `Hello`, `docs/contracts/`, `app/ui/src/features/devices`.

### 4. Translations (G-56)
All copy is English, but it is already centralised in `app/ui/src/copy.ts` and relative times use
`Intl`. Add an i18n layer (message catalogue keyed by id, locale from the OS) and extract the copy into
`locales/en.json` as the first catalogue.

### 5. Signed and notarised installers
`release.yml` builds unsigned bundles.
- Add macOS signing and notarisation (Developer ID certificate, notarytool) and Windows Authenticode
  signing for the msi and nsis installers. Both read their secrets from repository secrets.
- Prerequisite for the updater (2).
- Needs the owner's certificates and an Apple Developer account.

## Improvements

### 6. Automatic contract-drift check (H-05)
Nothing checks that the engine, the TypeScript client, the mock engine, the UI copy and
`docs/contracts/` agree on commands, events, error codes and link errors. A blocker (H-01) came from
exactly this drift.
- Add a Rust test that writes the canonical lists to a JSON fixture.
- Add a vitest that compares that fixture with the TS union types, `MockTransport` and `copy.ts`.
- Add a docs check that each name appears in `docs/contracts/daemon-ipc.md`.

### 7. Windows hook watchdog (H-18)
Windows silently removes a low-level hook whose callback exceeds `LowLevelHooksTimeout`, and nothing
reinstalls it. The macOS event tap already re-arms itself. Add a heartbeat that detects a dead hook
(no callbacks while input is clearly happening, or `SetWindowsHookEx` health) and reinstalls it.
Validate on Windows hardware.
- Area: `platform/src/windows/capture.rs`.

### 8. Accessibility checks (H-20)
Add `@axe-core/playwright` checks over the main routes (onboarding, Home, Computers, Keyboard &
Shortcuts, General, Diagnostics, tray) in light and dark, and fix the violations they find in
`design-system/`, not as one-off CSS.

### 9. Per-computer connection state (G-18)
`link_state` is still one global value, although every device already carries its own `state` and
`link_error`. Narrow the global value to engine health (running / not responding) and derive all
per-computer UI from the device fields.
- Contract change.

### 10. Architecture overview (H-12)
`docs/architecture/implementation-map.md` describes the removed V1 data path (pipeline, ownership,
hotkey runner, Primary/Secondary). Replace it with a current overview with a mermaid diagram:
engine library, router and roles, link and pairing, the Tauri shell, the headless daemon, the harness
and the live E2E.

### 11. Remove stale wording (H-13)
Text that still refers to the old pipeline, the hotkey runner, "Flutter↔daemon" or "slave":
- the README roadmap
- `daemon/README.md`
- `daemon/src/ipc/server.rs`
- `daemon/examples/drive_two_daemons.rs`
- `platform/src/windows/{mod,inject_translate}.rs`

### 12. Small cleanups
- H-17: document `open_main_window` (answered by the desktop shell, never the engine) in
  `docs/contracts/daemon-ipc.md`.
- H-19: `just lint` runs prettier twice (`scripts/dev.sh`).

### 13. Hardware validation
Run `docs/testing/physical-test-script.md` on real Windows, macOS and Linux machines. It covers what
has never run on real hardware:
- the Windows LL hooks and the macOS event tap
- the permission prompts
- the tray on each OS
- the installers
- pairing between two physical computers
- the original Windows → Mac bug scenario
