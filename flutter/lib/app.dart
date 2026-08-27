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
  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    trayManager.addListener(this);
    unawaited(_initWindowAndTray());
  }

  @override
  void dispose() {
    windowManager.removeListener(this);
    trayManager.removeListener(this);
    super.dispose();
  }

  /// Swallows any failure from these platform-channel calls rather than
  /// letting one propagate out of `initState` — there's no native
  /// `window_manager`/`tray_manager` implementation to talk to under
  /// `flutter test` (no docked tray icon there is expected, not a bug),
  /// and on a real desktop a tray/window setup failure shouldn't take
  /// the rest of the app down with it, the same "degrade gracefully, not
  /// fatally" contract the daemon's own hotkey runner already uses for a
  /// missing capture device.
  Future<void> _initWindowAndTray() async {
    try {
      await windowManager.setPreventClose(true);
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
      // No tray/window plugin available in this environment.
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
    unawaited(trayManager.popUpContextMenu());
  }

  @override
  Widget build(BuildContext context) {
    final platform = currentHostOs();
    final onboardingComplete = ref.watch(onboardingCompleteProvider);

    Widget onboarding() => OnboardingFlow(
      platform: platform,
      onDone: () {
        unawaited(_completeOnboarding());
      },
    );

    return Scaffold(
      body: Center(
        child: onboardingComplete.when(
          loading: () => const SizedBox.shrink(),
          // Can't tell whether onboarding has run before (no local
          // preferences plugin available) — fail safe by showing it
          // again rather than risking dropping straight into the
          // dashboard for someone who never onboarded at all.
          error: (_, _) => onboarding(),
          data: (complete) =>
              complete ? AppWindowShell(platform: platform) : onboarding(),
        ),
      ),
    );
  }

  Future<void> _completeOnboarding() async {
    await saveOnboardingComplete();
    ref.invalidate(onboardingCompleteProvider);
  }
}
