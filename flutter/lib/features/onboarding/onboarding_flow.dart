import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../core/platform_chrome.dart';
import '../../core/theme/flow_theme.dart';
import '../../core/theme/tokens.dart';
import '../../core/widgets/app_logo.dart';
import '../../core/widgets/glass_surface.dart';
import '../../core/widgets/window_chrome.dart';
import '../../domain/daemon_command_exception.dart';
import '../../domain/device.dart';
import '../../domain/pairing.dart';
import '../../domain/permission_status.dart';
import '../../state/repository_providers.dart';
import 'steps/done_step.dart';
import 'steps/pair_step.dart';
import 'steps/permission_step.dart';
import 'steps/welcome_step.dart';

const _stepCount = 4;
const _stepLabels = ['Welcome', 'Permission', 'Find your computer', 'All set'];

/// The 4-step first-launch flow (welcome, permission, pair, done), direct
/// implementation of the `isOnboarding` branch in `Cross-Device UI v2.
/// dc.html`: a 520px glass panel, animated step transitions, and dot
/// pagination.
///
/// [standalone] switches the outer chrome between two callers with very
/// different needs:
/// - `false` (the dev harness, `features/harness/dev_harness.dart`): the
///   original look — a fake `WindowChrome` title bar on the glass panel,
///   because the harness places it on top of a mock desktop wallpaper to
///   simulate what this window would look like *from the outside*, next
///   to every other platform's rendering, with no real OS window backing
///   it at all.
/// - `true` (the shipped app, `app.dart`'s `_RealApp`): this content
///   already lives inside a real OS window with its own real title bar —
///   drawing a second, fake one around it read as "a window inside a
///   window" (a real reported bug, not a stylistic nitpick). Standalone
///   mode drops `WindowChrome` entirely for a plain header carrying the
///   app mark and a "Step X of 4" label instead, so the required-setup
///   nature of the flow stays legible without impersonating a window
///   `_RealApp` already provides.
class OnboardingFlow extends ConsumerStatefulWidget {
  const OnboardingFlow({
    super.key,
    required this.platform,
    required this.onDone,
    this.standalone = false,
  });

  final HostOs platform;
  final VoidCallback onDone;
  final bool standalone;

  @override
  ConsumerState<OnboardingFlow> createState() => _OnboardingFlowState();
}

class _OnboardingFlowState extends ConsumerState<OnboardingFlow> {
  int _step = 0;
  bool _requestingPermission = false;
  String? _permissionError;

  void _goTo(int step) {
    setState(() => _step = step);
    if (step == 2) {
      final session = ref.read(pairingSessionProvider).valueOrNull;
      if (session == null || session.stage == PairingStage.idle) {
        ref.read(daemonRepositoryProvider).startPairing();
      }
    }
  }

  /// Drives the permission step's "Allow" button: shows a spinner while
  /// the request is in flight and a concrete error (with a retry — the
  /// button re-enables in the `finally`) instead of the button silently
  /// doing nothing when it fails. Distinct from [_openSystemSettings],
  /// which is the button's other job — actually opening the OS's privacy
  /// pane so there's somewhere for a real Accept/Deny prompt to appear.
  Future<void> _handleGrant() async {
    setState(() {
      _requestingPermission = true;
      _permissionError = null;
    });
    unawaited(_openSystemSettings());
    try {
      await ref.read(daemonRepositoryProvider).requestPermission();
    } on DaemonCommandException catch (e) {
      if (mounted) setState(() => _permissionError = e.message);
    } catch (_) {
      if (mounted) {
        setState(
          () => _permissionError =
              "Couldn't reach Flow's background service. Make sure it's "
              'running, then try again.',
        );
      }
    } finally {
      if (mounted) setState(() => _requestingPermission = false);
    }
  }

  /// Best-effort: opens the OS's own privacy/permission settings pane so
  /// there's an actual place for the user to flip the switch, rather than
  /// "Allow" being a button that visibly does nothing (the reported bug).
  /// Failures here are swallowed — the daemon-side grant above is still
  /// the source of truth for [PermissionStatus.granted], this is purely a
  /// navigation convenience, and there's no OS pane to open on Linux.
  Future<void> _openSystemSettings() async {
    final uri = switch (widget.platform) {
      HostOs.macos => Uri.parse(
        'x-apple.systempreferences:com.apple.preference.security'
        '?Privacy_Accessibility',
      ),
      HostOs.windows => Uri.parse('ms-settings:privacy-general'),
      HostOs.linux => null,
    };
    if (uri == null) return;
    try {
      await launchUrl(uri);
    } catch (_) {
      // No handler for the scheme in this environment (e.g. running in
      // CI, or a Linux desktop with no equivalent pane) — nothing else to
      // do about it.
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = FlowColors.of(context);
    final chrome = PlatformChrome.of(widget.platform);

    // Once pairing succeeds, move on to the Done step automatically —
    // onboarding doesn't need to wait for the pairing session's own
    // auto-return-to-idle timer the way the tray popover does.
    ref.listen(pairingSessionProvider, (previous, next) {
      final stage = next.valueOrNull?.stage;
      if (_step == 2 && stage == PairingStage.paired) {
        setState(() => _step = 3);
      }
    });

    final permissionState = ref.watch(permissionProvider);
    final permission = permissionState.valueOrNull;
    final session =
        ref.watch(pairingSessionProvider).valueOrNull ?? PairingSession.idle;
    final switchKeyLabel =
        ref.watch(settingsProvider).valueOrNull?.switchKey.label ?? '—';

    final header = widget.standalone
        ? _StandaloneHeader(palette: c, accent: c.accent, step: _step)
        : WindowChrome(
            controls: chrome.controls,
            title: 'Cross Device',
            background: c.chromeBg,
            border: c.border,
            textColor: c.text1,
            textSecondary: c.text2,
          );

    return GlassSurface(
      color: c.winGlass,
      border: c.border,
      blurSigma: 40,
      borderRadius: BorderRadius.circular(chrome.windowRadius()),
      boxShadow: c.shadow,
      child: SizedBox(
        width: 520,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            header,
            Padding(
              padding: const EdgeInsets.fromLTRB(34, 30, 34, 22),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  AnimatedSwitcher(
                    duration: const Duration(milliseconds: 240),
                    transitionBuilder: (child, animation) => FadeTransition(
                      opacity: animation,
                      child: SlideTransition(
                        position: animation.drive(
                          Tween(begin: const Offset(0, 0.02), end: Offset.zero),
                        ),
                        child: child,
                      ),
                    ),
                    child: KeyedSubtree(
                      key: ValueKey(_step),
                      child: switch (_step) {
                        0 => WelcomeStep(
                          palette: c,
                          onContinue: () => _goTo(1),
                        ),
                        1 => PermissionStep(
                          palette: c,
                          permission:
                              permission ??
                              PermissionStatus(
                                name: chrome.permissionName,
                                granted: false,
                              ),
                          isRequesting: _requestingPermission,
                          errorMessage: _permissionError,
                          connectionError:
                              permission == null && permissionState.hasError,
                          onGrant: () => unawaited(_handleGrant()),
                          onContinue: () => _goTo(2),
                        ),
                        2 => PairStep(
                          palette: c,
                          session: session,
                          onPair: (id) => ref
                              .read(daemonRepositoryProvider)
                              .pairWithCandidate(id),
                        ),
                        _ => DoneStep(
                          palette: c,
                          trayName: chrome.trayName,
                          switchKeyLabel: switchKeyLabel,
                          onDone: widget.onDone,
                        ),
                      },
                    ),
                  ),
                  const SizedBox(height: 18),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      for (var i = 0; i < _stepCount; i++)
                        AnimatedContainer(
                          duration: FlowMotion.hoverPress,
                          curve: FlowMotion.ease,
                          margin: const EdgeInsets.symmetric(horizontal: 3),
                          width: 5,
                          height: 5,
                          decoration: BoxDecoration(
                            color: i == _step ? c.accent : c.mat3,
                            shape: BoxShape.circle,
                          ),
                        ),
                    ],
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// [OnboardingFlow.standalone]'s header: the app mark plus a "Setup"
/// eyebrow and "Step X of 4" label, replacing [WindowChrome] so the panel
/// stops impersonating a second OS window inside `_RealApp`'s real one —
/// while still making it obvious at a glance that this is a required,
/// numbered setup flow rather than an ordinary dialog (a separate reported
/// complaint from the nested-window one).
class _StandaloneHeader extends StatelessWidget {
  const _StandaloneHeader({
    required this.palette,
    required this.accent,
    required this.step,
  });

  final FlowPalette palette;
  final Color accent;
  final int step;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return Padding(
      padding: const EdgeInsets.fromLTRB(26, 22, 26, 0),
      child: Row(
        children: [
          AppLogoMark(accent: accent),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text('SETUP', style: FlowType.sectionLabel(c.text3)),
                Text(
                  _stepLabels[step],
                  style: FlowType.body(c.text2, weight: FontWeight.w600),
                ),
              ],
            ),
          ),
          Text(
            'Step ${step + 1} of $_stepCount',
            style: FlowType.meta(c.text3),
          ),
        ],
      ),
    );
  }
}
