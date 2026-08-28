import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:tray_manager/tray_manager.dart';
import 'package:window_manager/window_manager.dart';

import 'core/theme/flow_theme.dart';
import 'data/onboarding_prefs.dart';
import 'domain/daemon_command_exception.dart';
import 'domain/daemon_link_state.dart';
import 'domain/device.dart';
import 'features/app_window/app_window_shell.dart';
import 'features/harness/dev_harness.dart';
import 'features/onboarding/onboarding_flow.dart';
import 'features/pairing/incoming_pairing_request_listener.dart';
import 'features/tray/tray_menu.dart';
import 'state/app_env.dart';
import 'state/repository_providers.dart';
import 'state/ui_mode.dart';
import 'state/ui_providers.dart';

/// Whether onboarding has ever completed, loaded once from
/// `data/onboarding_prefs.dart`. `_RealApp.onDone` calls
/// `ref.invalidate(onboardingCompleteProvider)` after persisting
/// completion, which re-runs this and flips the app over to the
/// dashboard — the standard Riverpod way to react to a one-shot async
/// write without a second, hand-rolled notifier.
final onboardingCompleteProvider = FutureProvider<bool>((ref) {
  return loadOnboardingComplete();
});

/// The real OS this build is running on — unlike the dev harness's
/// [HostOs] segmented control, the shipped app always renders its actual
/// platform's chrome, never a simulated one.
HostOs currentHostOs() {
  if (Platform.isMacOS) return HostOs.macos;
  if (Platform.isWindows) return HostOs.windows;
  return HostOs.linux;
}

/// Raises and focuses the real OS window. Top-level so surfaces outside
/// [_RealApp] — e.g. [IncomingPairingRequestListener] — can pull the
/// window forward without reaching into [_RealAppState]. Swallows any
/// failure from these platform-channel calls: there's no native
/// `window_manager` implementation under `flutter test`, and a raise
/// failure on a real desktop shouldn't take anything else down.
Future<void> showMainWindow() async {
  try {
    await windowManager.show();
    await windowManager.focus();
  } catch (_) {
    // No window plugin available in this environment.
  }
}

/// The side effect a tray menu row's [TrayAction] resolves to, decoupled
/// from the `tray_manager`/`window_manager` calls that carry it out so
/// the mapping itself is a pure, unit-testable function
/// ([resolveTrayAction]). [_RealAppState._runTrayAction] is the only
/// interpreter.
sealed class TrayActionEffect {
  const TrayActionEffect();
}

/// Ask the daemon to make [deviceId] the active input target. No window
/// change — switching devices from the tray is meant to be glanceable.
class SwitchDeviceEffect extends TrayActionEffect {
  const SwitchDeviceEffect(this.deviceId);
  final String deviceId;
}

/// Raise the window and land it on [section].
class ShowWindowEffect extends TrayActionEffect {
  const ShowWindowEffect(this.section);
  final AppSection section;
}

/// Kick off a pairing session, then raise the window on [section] so the
/// user can complete it there.
class StartPairingThenShowEffect extends TrayActionEffect {
  const StartPairingThenShowEffect(this.section);
  final AppSection section;
}

/// Quit the app for real (undo hide-to-tray, close the window).
class QuitEffect extends TrayActionEffect {
  const QuitEffect();
}

/// Pure mapping from a tray menu [TrayAction] to the [TrayActionEffect]
/// the app should run. Total over [TrayActionKind] — no default arm — so
/// a new action kind is a compile error here until it's handled.
TrayActionEffect resolveTrayAction(TrayAction action) => switch (action.kind) {
  TrayActionKind.switchDevice => SwitchDeviceEffect(action.switchDeviceId!),
  TrayActionKind.pairNewDevice => const StartPairingThenShowEffect(
    AppSection.dashboard,
  ),
  TrayActionKind.openDashboard => const ShowWindowEffect(AppSection.dashboard),
  TrayActionKind.openSettings => const ShowWindowEffect(AppSection.general),
  TrayActionKind.quitApp => const QuitEffect(),
};

/// App root. `--dart-define=FLOW_UI_MODE=harness` (`state/ui_mode.dart`)
/// still reaches [DevHarness] — the manual QA surface covering every
/// screen/state/platform combination without a running daemon or OS
/// tray, described in its own doc comment. Otherwise this is the shipped
/// app: [_RealApp] docks a real OS tray icon and window and drives
/// onboarding/dashboard off real daemon and local-preference state.
class FlowApp extends ConsumerWidget {
  const FlowApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final themeMode = ref.watch(themeModeProvider);
    return MaterialApp(
      title: 'Flow',
      debugShowCheckedModeBanner: false,
      themeMode: themeMode,
      theme: FlowTheme.light(),
      darkTheme: FlowTheme.dark(),
      home: uiMode == UiMode.harness ? const DevHarness() : const _RealApp(),
    );
  }
}

/// The shipped desktop app. Docks a real tray icon (`tray_manager`) with
/// a native context menu, and makes closing the window hide it rather
/// than quit — matching `vision.md` principle 7 ("the daemon keeps
/// working even if the UI is closed") now that there's a real window to
/// hide instead of always leaving a mock desktop on screen. Shows
/// [OnboardingFlow] until onboarding has completed at least once,
/// [AppWindowShell] afterward.
///
/// The window keeps its native title bar/frame for now rather than the
/// design mockup's fully frameless glass look (`WindowChrome` is still
/// only ever decorative — see its own doc comment) — a real, movable,
/// closable OS window is the priority for this pass; a custom frameless
/// shell with its own drag region is a follow-up, not attempted here to
/// avoid destabilizing the dashboard/popover's existing interactive
/// controls with a drag-gesture region layered over them.
class _RealApp extends ConsumerStatefulWidget {
  const _RealApp();

  @override
  ConsumerState<_RealApp> createState() => _RealAppState();
}

class _RealAppState extends ConsumerState<_RealApp>
    with WindowListener, TrayListener {
  /// Guards [_setUpTray] so it only ever runs once — it's called both from
  /// [build] (in case onboarding was already complete when the app
  /// launched) and from the [onboardingCompleteProvider] listener (once it
  /// flips to complete), and re-docking the same tray icon twice is wasted
  /// work at best.
  bool _trayReady = false;

  /// Which section the window should open on next time it's raised from a
  /// tray action. Fed into [AppWindowShell.initialSection] (with a
  /// matching [ValueKey] so a change actually remounts the shell on the
  /// new section) — the tray's Dashboard/Settings rows are the only
  /// things that write it.
  AppSection _pendingSection = AppSection.dashboard;

  /// Bumped every time a tray action targets a section, so the shell
  /// remounts even when [_pendingSection] is unchanged — e.g. tray→Settings
  /// while the user has already navigated in-app away from General. Folded
  /// into [AppWindowShell]'s [ValueKey] alongside [_pendingSection].
  int _sectionEpoch = 0;

  /// Live subscriptions that keep the native tray menu in step with
  /// daemon state — the menu lists switchable devices and the link
  /// status, both of which change without any `build` of this widget.
  /// Registered once from [build] (after the tray is set up), cancelled
  /// in [dispose].
  ProviderSubscription<AsyncValue<List<Device>>>? _devicesSub;
  ProviderSubscription<AsyncValue<DaemonLinkState>>? _linkSub;

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    trayManager.addListener(this);
    unawaited(_initWindow());
  }

  @override
  void dispose() {
    _devicesSub?.close();
    _linkSub?.close();
    windowManager.removeListener(this);
    trayManager.removeListener(this);
    super.dispose();
  }

  /// Swallows any failure from these platform-channel calls rather than
  /// letting one propagate out of `initState` — there's no native
  /// `window_manager` implementation to talk to under `flutter test`, and
  /// on a real desktop a window setup failure shouldn't take the rest of
  /// the app down with it, the same "degrade gracefully, not fatally"
  /// contract the daemon's own hotkey runner already uses for a missing
  /// capture device.
  ///
  /// Skips `setPreventClose` entirely under `isDevelopmentEnv`
  /// (`--dart-define=FLOW_ENV=development`): with it set, the window's
  /// real close button just hides the app (see [_hideInsteadOfClosing]),
  /// by design, so the tray keeps working — but every `flutter run`
  /// during local iteration then launches a *new* process while the
  /// previous one is still alive in the background, hidden. macOS's own
  /// Launch Services then can't foreground the freshly built app (a real,
  /// reproduced symptom: `flutter run` logging "Failed to foreground app;
  /// open returned 1"), and whoever is testing ends up looking at
  /// whichever stale instance still has focus/the tray icon instead of
  /// the one that just launched — easy to mistake for "the daemon
  /// connection is broken" when it's actually a leftover process. A dev
  /// build's close button quitting for real, like any other desktop app
  /// in development, avoids that trap; the shipped app's hide-to-tray
  /// behavior is untouched.
  Future<void> _initWindow() async {
    if (isDevelopmentEnv) return;
    try {
      await windowManager.setPreventClose(true);
    } catch (_) {
      // No window plugin available in this environment.
    }
  }

  /// Docks the tray/menu-bar icon. Deliberately *not* called until
  /// onboarding has completed at least once: a tray icon the user can
  /// click into a half-set-up app (no permission granted, nothing paired
  /// yet) is more confusing than reassuring, and every tray action below
  /// assumes a real window and daemon state already exist to act on.
  Future<void> _setUpTray() async {
    if (_trayReady) return;
    _trayReady = true;
    try {
      await trayManager.setIcon(
        Platform.isWindows ? 'assets/tray_icon.ico' : 'assets/tray_icon.png',
      );
      await trayManager.setToolTip('Flow');
      await _rebuildTrayMenu();
    } catch (_) {
      // No tray plugin available in this environment.
      _trayReady = false;
    }
  }

  /// Rebuilds the native context menu from current daemon state via the
  /// pure [buildTrayMenu]. Called on tray setup, whenever the device list
  /// or link state changes (see [_devicesSub]/[_linkSub]), and right
  /// before the menu is popped. A no-op until the tray icon is actually
  /// docked; the `setContextMenu` call is guarded so envs without the
  /// plugin (e.g. `flutter test`) fall through harmlessly.
  Future<void> _rebuildTrayMenu() async {
    if (!_trayReady) return;
    final link =
        ref.read(linkStateProvider).valueOrNull ?? DaemonLinkState.connecting;
    final devices = ref.read(devicesProvider).valueOrNull ?? const <Device>[];
    final entries = buildTrayMenu(
      link: link,
      devices: devices,
      localDeviceId: 'd1',
    );
    try {
      await trayManager.setContextMenu(
        Menu(
          items: [
            for (final e in entries)
              if (e.isSeparator)
                MenuItem.separator()
              else
                MenuItem(
                  key: e.label,
                  label: e.label,
                  disabled: !e.enabled,
                  onClick: e.action == null
                      ? null
                      : (_) => unawaited(_runTrayAction(e.action!)),
                ),
          ],
        ),
      );
    } catch (_) {
      // No tray plugin available in this environment.
    }
  }

  /// Interprets one tray menu click: maps the [TrayAction] to a
  /// [TrayActionEffect] via the pure [resolveTrayAction], then carries it
  /// out against the daemon/window. Daemon rejections surface as a toast
  /// (switch) or are swallowed (pairing — "already pairing" is fine).
  Future<void> _runTrayAction(TrayAction action) async {
    final repo = ref.read(daemonRepositoryProvider);
    switch (resolveTrayAction(action)) {
      case SwitchDeviceEffect(:final deviceId):
        try {
          await repo.switchActiveDevice(deviceId);
        } on DaemonCommandException catch (e) {
          ref.read(toastProvider.notifier).show(e.message);
        }
      case ShowWindowEffect(:final section):
        setState(() {
          _pendingSection = section;
          _sectionEpoch++;
        });
        await showMainWindow();
      case StartPairingThenShowEffect(:final section):
        setState(() {
          _pendingSection = section;
          _sectionEpoch++;
        });
        try {
          await repo.startPairing();
        } on DaemonCommandException {
          // Already pairing — fine, the window still comes forward.
        }
        await showMainWindow();
      case QuitEffect():
        await _quit();
    }
  }

  Future<void> _quit() async {
    // Undo `setPreventClose` first — otherwise `close()` would just hide
    // the window again via `onWindowClose` below, the same as clicking
    // the window's own close button.
    await windowManager.setPreventClose(false);
    await windowManager.close();
  }

  @override
  void onWindowClose() {
    unawaited(_hideInsteadOfClosing());
  }

  Future<void> _hideInsteadOfClosing() async {
    if (await windowManager.isPreventClose()) {
      await windowManager.hide();
    }
  }

  @override
  void onTrayIconMouseDown() {
    // Left-click now opens the same native menu as right-click (built
    // from live daemon state) rather than toggling the window — the tray
    // is a control surface, not just a show/hide switch. The `_trayReady`
    // guard keeps a stray event from a torn-down (or not-yet-set-up) icon
    // from reaching the plugin during/around onboarding.
    if (!_trayReady) return;
    unawaited(_openTrayMenu());
  }

  @override
  void onTrayIconRightMouseDown() {
    if (!_trayReady) return;
    unawaited(_openTrayMenu());
  }

  Future<void> _openTrayMenu() async {
    await _rebuildTrayMenu();
    try {
      await trayManager.popUpContextMenu();
    } catch (_) {
      // No tray plugin available in this environment.
    }
  }

  @override
  Widget build(BuildContext context) {
    final platform = currentHostOs();
    final onboardingComplete = ref.watch(onboardingCompleteProvider);
    final c = FlowColors.of(context);

    // Dock the tray icon the moment onboarding is (or already was, on a
    // returning launch) complete — never before, so there's nothing in
    // the menu bar/system tray for the user to click into while setup is
    // still in progress (see `_setUpTray`'s own doc comment).
    if (onboardingComplete.valueOrNull == true) {
      unawaited(_setUpTray());
    }

    // Keep the native tray menu in step with daemon state — the device
    // list and link status both change without rebuilding this widget.
    // Registered once; cancelled in `dispose`.
    _devicesSub ??= ref.listenManual(
      devicesProvider,
      (_, _) => unawaited(_rebuildTrayMenu()),
    );
    _linkSub ??= ref.listenManual(
      linkStateProvider,
      (_, _) => unawaited(_rebuildTrayMenu()),
    );

    Widget onboarding() => OnboardingFlow(
      platform: platform,
      standalone: true,
      onDone: () {
        unawaited(_completeOnboarding());
      },
    );

    final content = onboardingComplete.when(
      loading: () => const SizedBox.shrink(),
      // Can't tell whether onboarding has run before (no local
      // preferences plugin available) — fail safe by showing it
      // again rather than risking dropping straight into the
      // dashboard for someone who never onboarded at all.
      error: (_, _) => onboarding(),
      data: (complete) => complete
          ? AppWindowShell(
              key: ValueKey((_pendingSection, _sectionEpoch)),
              platform: platform,
              standalone: true,
              initialSection: _pendingSection,
            )
          : onboarding(),
    );

    // The onboarding/dashboard content already carries its own glass
    // panels; behind them this real OS window wants a soft, unmistakably
    // "desktop-ish" backdrop rather than a flat near-black rectangle —
    // reusing the same wallpaper gradient the dev harness paints behind
    // its mock desktop, since that's exactly the surface a floating panel
    // is designed to sit on.
    return Scaffold(
      body: Container(
        decoration: BoxDecoration(gradient: c.wallpaper),
        child: IncomingPairingRequestListener(
          onShouldSurfaceWindow: () => unawaited(showMainWindow()),
          child: Center(child: content),
        ),
      ),
    );
  }

  Future<void> _completeOnboarding() async {
    await saveOnboardingComplete();
    ref.invalidate(onboardingCompleteProvider);
  }
}
