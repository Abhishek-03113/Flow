import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/platform_chrome.dart';
import '../../core/theme/flow_theme.dart';
import '../../core/theme/tokens.dart';
import '../../core/widgets/primitives.dart';
import '../../core/widgets/toast.dart';
import '../../data/mock_daemon_repository.dart';
import '../../domain/daemon_link_state.dart';
import '../../domain/device.dart';
import '../../state/repository_providers.dart';
import '../../state/ui_providers.dart';
import '../app_window/app_window_shell.dart';
import '../onboarding/onboarding_flow.dart';
import '../pairing/incoming_pairing_request_listener.dart';
import '../tray/tray_popover.dart';

enum HarnessView { menuBar, appWindow, firstLaunch, allPlatforms }

const _linkStateLabels = {
  DaemonLinkState.connected: 'Connected',
  DaemonLinkState.connecting: 'Connecting',
  DaemonLinkState.reconnecting: 'Reconnecting',
  DaemonLinkState.disconnected: 'Disconnected',
  DaemonLinkState.error: 'Error',
  DaemonLinkState.permissionRequired: 'Permission',
};

/// Dev-only manual QA harness: the control strip from the Claude Design
/// canvas (View / Connection / Platform / Theme) driving the exact same
/// providers every real screen reads, so every combination of state,
/// platform, and theme is reachable in one window without a real daemon
/// or a real OS tray. Not the shipped app's home anymore — a real daemon
/// and real tray/window docking now exist (`app.dart`'s `_RealApp`) — but
/// still reachable on demand via `--dart-define=FLOW_UI_MODE=harness`
/// (`state/ui_mode.dart`), since simulating every platform/state
/// combination side by side is still useful for manual QA that a single
/// real window and a single real daemon connection can't offer.
class DevHarness extends ConsumerStatefulWidget {
  const DevHarness({super.key});

  @override
  ConsumerState<DevHarness> createState() => _DevHarnessState();
}

class _DevHarnessState extends ConsumerState<DevHarness> {
  HarnessView _view = HarnessView.menuBar;
  HostOs _platform = HostOs.macos;

  @override
  Widget build(BuildContext context) {
    final c = FlowColors.of(context);
    final linkState =
        ref.watch(linkStateProvider).valueOrNull ?? DaemonLinkState.connecting;
    final themeMode = ref.watch(themeModeProvider);

    return Scaffold(
      backgroundColor: c.shell,
      body: IncomingPairingRequestListener(
        child: Stack(
          children: [
            Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 18,
                    vertical: 10,
                  ),
                  decoration: BoxDecoration(
                    color: c.shell,
                    border: Border(bottom: BorderSide(color: c.hairline)),
                  ),
                  child: SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.center,
                      children: [
                        Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Container(
                              width: 16,
                              height: 16,
                              decoration: BoxDecoration(
                                color: c.accent,
                                borderRadius: BorderRadius.circular(5),
                              ),
                            ),
                            const SizedBox(width: 8),
                            Text(
                              'Cross Device',
                              style: FlowType.body(
                                c.text1,
                                weight: FontWeight.w700,
                              ).copyWith(fontSize: 12.5),
                            ),
                            const SizedBox(width: 18),
                          ],
                        ),
                        _labeled(
                          'View',
                          c,
                          FlowSegmentedControl<HarnessView>(
                            segments: const [
                              FlowSegment(
                                value: HarnessView.menuBar,
                                label: 'Menu Bar',
                              ),
                              FlowSegment(
                                value: HarnessView.appWindow,
                                label: 'App Window',
                              ),
                              FlowSegment(
                                value: HarnessView.firstLaunch,
                                label: 'First Launch',
                              ),
                              FlowSegment(
                                value: HarnessView.allPlatforms,
                                label: 'All Platforms',
                              ),
                            ],
                            selected: _view,
                            onChanged: (v) => setState(() => _view = v),
                            background: c.mat1,
                            selectedBackground: c.accent,
                            selectedForeground: c.accentText,
                            unselectedForeground: c.text2,
                          ),
                        ),
                        _labeled(
                          'Connection',
                          c,
                          FlowSegmentedControl<DaemonLinkState>(
                            segments: [
                              for (final entry in _linkStateLabels.entries)
                                FlowSegment(
                                  value: entry.key,
                                  label: entry.value,
                                ),
                            ],
                            selected: linkState,
                            onChanged: (s) => (ref.read(
                              daemonRepositoryProvider,
                            ) as MockDaemonRepository).debugSetLinkState(s),
                            background: c.mat1,
                            selectedBackground: c.accent,
                            selectedForeground: c.accentText,
                            unselectedForeground: c.text2,
                          ),
                        ),
                        _labeled(
                          'Platform',
                          c,
                          FlowSegmentedControl<HostOs>(
                            segments: const [
                              FlowSegment(value: HostOs.macos, label: 'macOS'),
                              FlowSegment(
                                value: HostOs.windows,
                                label: 'Windows',
                              ),
                              FlowSegment(value: HostOs.linux, label: 'Linux'),
                            ],
                            selected: _platform,
                            onChanged: (p) => setState(() => _platform = p),
                            background: c.mat1,
                            selectedBackground: c.accent,
                            selectedForeground: c.accentText,
                            unselectedForeground: c.text2,
                          ),
                        ),
                        _labeled(
                          'Theme',
                          c,
                          FlowSegmentedControl<ThemeMode>(
                            segments: const [
                              FlowSegment(value: ThemeMode.dark, label: 'Dark'),
                              FlowSegment(
                                value: ThemeMode.light,
                                label: 'Light',
                              ),
                            ],
                            selected: themeMode,
                            onChanged: (m) =>
                                ref.read(themeModeProvider.notifier).state = m,
                            background: c.mat1,
                            selectedBackground: c.accent,
                            selectedForeground: c.accentText,
                            unselectedForeground: c.text2,
                          ),
                        ),
                        ElevatedButton(
                          onPressed: () =>
                              (ref.read(daemonRepositoryProvider)
                                      as MockDaemonRepository)
                                  .simulateIncomingPairingRequest(
                                    deviceName: 'Office Mac Mini',
                                    deviceOs: HostOs.macos,
                                  ),
                          child: const Text(
                            'Simulate incoming pairing request',
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
                Expanded(
                  child: switch (_view) {
                    HarnessView.allPlatforms => _AllPlatformsCompare(
                      palette: c,
                    ),
                    _ => _SingleDesktop(
                      palette: c,
                      platform: _platform,
                      view: _view,
                    ),
                  },
                ),
              ],
            ),
            const ToastOverlay(),
          ],
        ),
      ),
    );
  }

  Widget _labeled(String label, FlowPalette c, Widget control) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          label,
          style: FlowType.sectionLabel(c.text3).copyWith(fontSize: 10.5),
        ),
        const SizedBox(width: 8),
        control,
        const SizedBox(width: 18),
      ],
    );
  }
}

/// A mock desktop: wallpaper + a minimal system bar with a tray icon
/// that toggles the popover, or the app window / onboarding centered.
class _SingleDesktop extends ConsumerWidget {
  const _SingleDesktop({
    required this.palette,
    required this.platform,
    required this.view,
  });

  final FlowPalette palette;
  final HostOs platform;
  final HarnessView view;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = palette;
    final chrome = PlatformChrome.of(platform);
    final trayOpen = ref.watch(trayOpenProvider);

    return Container(
      decoration: BoxDecoration(gradient: c.wallpaper),
      child: Stack(
        children: [
          if (view == HarnessView.menuBar)
            Positioned(
              left: 0,
              right: 0,
              top: chrome.barPosition == BarPosition.top ? 0 : null,
              bottom: chrome.barPosition == BarPosition.bottom ? 0 : null,
              child: Container(
                height: chrome.barHeight,
                color: c.barBg,
                padding: const EdgeInsets.symmetric(horizontal: 14),
                child: Row(
                  children: [
                    Text(
                      'Cross Device (dev harness)',
                      style: FlowType.meta(c.text1),
                    ),
                    const Spacer(),
                    GestureDetector(
                      onTap: () =>
                          ref.read(trayOpenProvider.notifier).state = !trayOpen,
                      child: Container(
                        width: 15,
                        height: 15,
                        decoration: BoxDecoration(
                          color: trayOpen ? c.accent : c.text2,
                          borderRadius: BorderRadius.circular(5),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          if (view == HarnessView.menuBar && trayOpen)
            Positioned(
              right: 14,
              top: chrome.barPosition == BarPosition.top
                  ? chrome.barHeight + 8
                  : null,
              bottom: chrome.barPosition == BarPosition.bottom
                  ? chrome.barHeight + 8
                  : null,
              child: TrayPopover(
                platform: platform,
                onOpenDashboard: () =>
                    ref.read(trayOpenProvider.notifier).state = false,
                onOpenSettings: () =>
                    ref.read(trayOpenProvider.notifier).state = false,
              ),
            ),
          if (view == HarnessView.appWindow)
            Center(child: AppWindowShell(platform: platform)),
          if (view == HarnessView.firstLaunch)
            Center(
              child: OnboardingFlow(platform: platform, onDone: () {}),
            ),
        ],
      ),
    );
  }
}

/// The "All Platforms" comparison strip: three framed mini-desktops,
/// each with its own bar and popover anchored at the correct edge.
class _AllPlatformsCompare extends StatelessWidget {
  const _AllPlatformsCompare({required this.palette});

  final FlowPalette palette;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(26, 30, 26, 40),
      child: Wrap(
        spacing: 26,
        runSpacing: 26,
        alignment: WrapAlignment.center,
        children: [
          for (final os in HostOs.values) _PlatformFrame(palette: c, os: os),
        ],
      ),
    );
  }
}

class _PlatformFrame extends StatelessWidget {
  const _PlatformFrame({required this.palette, required this.os});

  final FlowPalette palette;
  final HostOs os;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    final chrome = PlatformChrome.of(os);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          _osLabel(os),
          style: FlowType.body(
            c.text1,
            weight: FontWeight.w700,
          ).copyWith(fontSize: 13),
        ),
        Text(chrome.trayName, style: FlowType.meta(c.text3)),
        const SizedBox(height: 10),
        Container(
          width: 360,
          height: 470,
          clipBehavior: Clip.antiAlias,
          decoration: BoxDecoration(
            gradient: c.wallpaper,
            borderRadius: BorderRadius.circular(14),
            border: Border.all(color: c.border),
            boxShadow: c.shadow,
          ),
          child: Stack(
            children: [
              Positioned(
                left: 0,
                right: 0,
                top: chrome.barPosition == BarPosition.top ? 0 : null,
                bottom: chrome.barPosition == BarPosition.bottom ? 0 : null,
                child: Container(height: chrome.barHeight, color: c.barBg),
              ),
              Positioned(
                right: 14,
                top: chrome.barPosition == BarPosition.top
                    ? chrome.barHeight + 8
                    : null,
                bottom: chrome.barPosition == BarPosition.bottom
                    ? chrome.barHeight + 8
                    : null,
                child: Transform.scale(
                  scale: 0.9,
                  alignment: chrome.barPosition == BarPosition.top
                      ? Alignment.topRight
                      : Alignment.bottomRight,
                  child: TrayPopover(platform: os),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

String _osLabel(HostOs os) => switch (os) {
  HostOs.macos => 'macOS',
  HostOs.windows => 'Windows',
  HostOs.linux => 'Linux',
};
