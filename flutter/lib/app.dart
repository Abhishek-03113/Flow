import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:tray_manager/tray_manager.dart';
import 'package:window_manager/window_manager.dart';

import 'core/theme/flow_theme.dart';
import 'data/onboarding_prefs.dart';
import 'domain/device.dart';
import 'features/app_window/app_window_shell.dart';
import 'features/harness/dev_harness.dart';
import 'features/onboarding/onboarding_flow.dart';
import 'state/app_env.dart';
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

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    trayManager.addListener(this);
    unawaited(_initWindow());
  }

  @override
  void dispose() {
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
      await trayManager.setContextMenu(
        Menu(
          items: [
            MenuItem(
              key: 'open',
              label: 'Open Flow',
              onClick: (_) => unawaited(_showWindow()),
            ),
            MenuItem.separator(),
            MenuItem(
              key: 'quit',
              label: 'Quit Flow',
              onClick: (_) => unawaited(_quit()),
            ),
          ],
        ),
      );
    } catch (_) {
      // No tray plugin available in this environment.
      _trayReady = false;
    }
  }

  Future<void> _showWindow() async {
    await windowManager.show();
    await windowManager.focus();
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
    // A stray event from a tray icon that's already been torn down (or,
    // defensively, one that somehow fires before `_setUpTray` ever ran)
    // should never reach into `window_manager` — this is the guard that
    // keeps a tray click from crashing the app during/around onboarding.
    if (!_trayReady) return;
    unawaited(_toggleWindow());
  }

  Future<void> _toggleWindow() async {
    if (await windowManager.isVisible()) {
      await windowManager.hide();
    } else {
      await _showWindow();
    }
  }

  @override
  void onTrayIconRightMouseDown() {
    if (!_trayReady) return;
    unawaited(trayManager.popUpContextMenu());
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
      data: (complete) =>
          complete
              ? AppWindowShell(platform: platform, standalone: true)
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
        child: Center(child: content),
      ),
    );
  }

  Future<void> _completeOnboarding() async {
    await saveOnboardingComplete();
    ref.invalidate(onboardingCompleteProvider);
  }
}
