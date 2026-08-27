# Flow UI

Flutter control-plane app for Flow: a real OS tray icon/window, first-launch onboarding, and the dashboard/settings window. See [`docs/product/vision.md`](../docs/product/vision.md) for the product architecture and [`docs/contracts/`](../docs/contracts/) for the Flutter↔daemon contract, satisfied by both `lib/data/ipc_daemon_repository.dart` (the default, a real `flow-daemon` process over local IPC) and `lib/data/mock_daemon_repository.dart` (UI-only development without a daemon — see "Running without a daemon" below).

## Running it

```sh
# in one terminal, from the repo root
cargo run -p flow-daemon

# in another terminal
flutter pub get
flutter run -d linux   # or -d macos / -d windows, depending on your machine
```

This docks a real tray icon (`tray_manager`) and opens a real window (`window_manager`): closing the window hides it rather than quitting — the tray icon's left-click toggles it back, right-click opens a native "Open Flow" / "Quit Flow" menu, matching `vision.md` principle 7 ("the daemon keeps working even if the UI is closed"). On first launch you land in the onboarding flow (welcome → permission → pair → done); afterward, the window opens straight to the dashboard. Onboarding-completed is a local preference (`lib/data/onboarding_prefs.dart`, `shared_preferences`) — a UI-only concern, not part of the daemon contract, so it's per-machine and never round-trips through `flow-daemon`.

By default this expects a real `flow-daemon` already listening on `127.0.0.1:47823` (`flow_core::ipc::IPC_PORT`) — without one, every provider stays in `AsyncLoading`/errors out. See "Running without a daemon" below to work on the UI alone.

On macOS, `flutter run` (Debug/Profile) builds with App Sandbox off (`macos/Runner/DebugProfile.entitlements`) specifically so this works with zero permission friction — a sandboxed build denies the WebSocket connect to `flow-daemon` outright (`SocketException: Operation not permitted`, easy to mistake for "the daemon isn't running" even when it is) and can't read `~/.flow/ipc.token` either, since that path is outside the sandbox container. `Release.entitlements` is unaffected. If you still see that error right after pulling an entitlements change, run `flutter clean` first — Xcode's incremental build doesn't always re-sign on an entitlements-only diff.

Pairing note: the daemon only accepts an incoming pairing request while *its* user also has pairing open (`daemon/README.md`, "Pairing consent window"), so pairing two machines means pressing "Pair a device" on both, not just the initiating one.

The window keeps its native OS title bar/frame for now, not the design mockups' fully frameless glass look — `WindowChrome` (`lib/core/widgets/window_chrome.dart`) is still purely decorative (its traffic-light/win/gnome buttons don't do anything, and there's no custom drag region), and the window is a single fixed size shared by onboarding and the dashboard rather than resizing to fit each. A custom frameless shell is a reasonable follow-up, deliberately not attempted alongside this pass to avoid layering an untested drag-gesture region over the dashboard/popover's existing interactive controls.

## Running without a daemon

Pass `--dart-define=FLOW_DAEMON_MODE=mock` to swap `daemonRepositoryProvider` back to `MockDaemonRepository` — no daemon required, same seed data and pairing timings as before:

```sh
flutter run -d linux --dart-define=FLOW_DAEMON_MODE=mock
```

`flutter test` never depends on either default: every widget test overrides `daemonRepositoryProvider` explicitly with a `MockDaemonRepository` instance rather than reading the build-time flag, so the suite stays deterministic regardless of which backend `flutter run` defaults to.

## The dev harness

`--dart-define=FLOW_UI_MODE=harness` (`lib/state/ui_mode.dart`) reaches the original manual-QA surface instead of the shipped app — a control strip (View / Connection / Platform / Theme) driving the same providers every real screen reads, letting you reach every surface/state/platform combination in one window without a daemon or a real OS tray:

```sh
flutter run -d linux --dart-define=FLOW_UI_MODE=harness
```

- **Menu Bar** — the tray popover, anchored to a minimal mock desktop bar.
- **App Window** — the dashboard/settings window.
- **First Launch** — the onboarding flow.
- **All Platforms** — macOS/Windows/Linux tray popovers side by side.

The two dart-defines are independent: `FLOW_UI_MODE=harness --dart-define=FLOW_DAEMON_MODE=ipc` drives the harness's real screens against a real daemon, for instance.

## Testing against a real daemon

```sh
# in one terminal, from the repo root
cargo run -p flow-daemon

# in another terminal
cd flutter
flutter run -d linux --dart-define=FLOW_DAEMON_MODE=ipc
```

`test/data/ipc_daemon_repository_manual_test.dart` goes one step further: the same 13 scenarios `mock_daemon_repository_test.dart` proves against the mock, run against `IpcDaemonRepository` and a **real** `flow-daemon` process, confirming the two implementations are externally indistinguishable. Tagged `manual` (`dart_test.yaml` skips it by default — never part of a plain `flutter test`); run it explicitly per the recipe in its own doc comment (`flutter test --tags manual --run-skipped ...`, against a freshly-started daemon).

`test/e2e/daemon_ui_flow_e2e_test.dart` goes further still: full daemon-to-UI coverage, driving the real production screens (`TrayPopover`, `AppWindowShell`, `OnboardingFlow`) through actual taps against a real `flow-daemon` process it starts and stops itself — no manually-started daemon, no second terminal. It automates `docs/testing/manual-testing-strategy.md`'s Tier 0 checklist end to end, including restart persistence and a mid-session daemon kill. Tagged `e2e`; run it with `flutter test --tags e2e --run-skipped test/e2e/daemon_ui_flow_e2e_test.dart` (needs `cargo` on `PATH` and `127.0.0.1:47823` free — see the file's own doc comment).

## Testing and linting

```sh
flutter analyze
dart format --output=none --set-exit-if-changed lib test
flutter test
```

All three are expected to be clean. `flutter test` is real-time-based, not instant — the pairing flow and toast dismissals run through actual (fake-clock-advanced) delays, so the full suite takes on the order of 10-15 seconds.

## Layout

- `lib/domain/` — pure-Dart contract types and the `DaemonRepository` interface (`docs/contracts/data-model.md`, `daemon-ipc.md`). No Flutter dependency.
- `lib/data/` — two `DaemonRepository` implementations: `IpcDaemonRepository` (default, a real `flow-daemon` over WebSocket; `ipc_codec.dart` holds its JSON<->domain-type conversions, `replay_channel.dart` the late-subscriber-replay primitive both implementations share for their `watch*` streams) and `MockDaemonRepository` (UI-only development); `onboarding_prefs.dart` is the local, daemon-independent "has onboarding ever completed" flag.
- `lib/state/` — Riverpod providers. `repository_providers.dart` picks the daemon backend (`daemonRepositoryProvider`, see "Running without a daemon" above); `ui_mode.dart` picks the shipped app vs. the dev harness (see "The dev harness" above); `ui_providers.dart` holds UI-only state (theme, tray-open, toasts) that never mixes with daemon state.
- `lib/core/` — design tokens/theme (`theme/`), platform metadata (`platform_chrome.dart`), and shared presentational widgets (`widgets/`).
- `lib/features/` — the actual screens: `tray/`, `onboarding/`, `app_window/`, and the dev-only `harness/`.
- `lib/app.dart`, `lib/main.dart` — entry point: `main.dart` initializes `window_manager` (skipped for the harness, which wants its own mock-desktop-sized window instead of a real docked one); `app.dart`'s `FlowApp` picks harness vs. shipped app, and `_RealApp` owns the tray icon, the window-hide-on-close behavior, and the onboarding/dashboard switch.
- `assets/tray_icon.png` / `assets/tray_icon.ico` — the tray icon (copies of the generated app icon; see "What's not here yet" below).

## What's not here yet

- A custom-designed tray icon and app icon — `assets/tray_icon.{png,ico}` are copies of the placeholder icon `flutter create` generated (`macos/Runner/Assets.xcassets`, `windows/runner/resources/app_icon.ico`), not real Flow branding.
- Real Flow branding: `assets/flow_logo.png` is a flat placeholder (solid accent fill, no mark) — `core/widgets/app_logo.dart`'s `AppLogo`/`AppLogoMark` already render it everywhere branding shows up (onboarding's welcome step and standalone header), so dropping the real artwork in at the same path is a straight asset swap with no code changes.
- A frameless custom window shell matching the design mockups exactly (native title bar today — see "Running it" above) and per-content window sizing (one fixed size shared by onboarding and the dashboard).
- Functional close/minimize/maximize buttons inside `WindowChrome` — still decorative; the real window controls are the native OS title bar's.
- Native platform runners (`macos/`, `windows/`, `linux/`) were generated by `flutter create` and haven't been customized beyond the tray/window wiring above (entitlements, code signing, etc.) — they're enough to run the app, not to ship it.

## Environment note

This project's Flutter SDK, in whatever session built it, was vendored locally (not assumed to be pre-installed) — a fresh clone still needs its own working Flutter install (`flutter --version` should report a recent stable release; this project was last verified against 3.47.1) with the `linux-desktop`/`macos-desktop`/`windows-desktop` config enabled as needed (`flutter config --enable-<platform>-desktop`).
